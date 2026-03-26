from __future__ import annotations

import importlib
import os
import stat
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src"
if str(SRC) not in sys.path:
    sys.path.insert(0, str(SRC))

import fastapi_doctor.app_loader as app_loader_module  # noqa: E402
import fastapi_doctor.native_core as native_core_module  # noqa: E402
import fastapi_doctor.project as project_module  # noqa: E402
import fastapi_doctor.runner as runner_module  # noqa: E402


def _write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)


def _write_executable(path: Path, content: str) -> None:
    _write(path, content)
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def _reload_runner(monkeypatch, repo_root: Path, *, code_dir: str | None = None):
    monkeypatch.setenv("DOCTOR_REPO_ROOT", str(repo_root))
    if code_dir is None:
        monkeypatch.delenv("DOCTOR_CODE_DIR", raising=False)
    else:
        monkeypatch.setenv("DOCTOR_CODE_DIR", code_dir)
    monkeypatch.delenv("DOCTOR_IMPORT_ROOT", raising=False)
    monkeypatch.delenv("DOCTOR_APP_MODULE", raising=False)
    importlib.reload(project_module)
    importlib.reload(app_loader_module)
    return importlib.reload(runner_module)


def _issue_fingerprint(issue) -> tuple[str, str, int, str]:
    return (issue.check, issue.path, issue.line, issue.message)


def _prefer_repo_debug_binary(monkeypatch) -> None:
    override = os.environ.get("FASTAPI_DOCTOR_NATIVE_BINARY", "").strip()
    binary = Path(override) if override else None
    monkeypatch.setattr(
        native_core_module,
        "_resolve_binary",
        lambda: binary if binary and binary.is_file() else None,
    )


def test_override_binary_is_used_when_versions_match(monkeypatch, tmp_path: Path) -> None:
    binary = tmp_path / "fastapi-doctor-native"
    _write_executable(
        binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf '1.2.3\\n'\n  exit 0\nfi\nexit 1\n",
    )

    native_core_module._VERSION_OK_CACHE.clear()
    monkeypatch.setenv("FASTAPI_DOCTOR_NATIVE_BINARY", str(binary))
    monkeypatch.setattr(native_core_module, "_package_version", lambda: "1.2.3")

    assert native_core_module._resolve_binary() == binary
    assert "override native binary" in native_core_module.last_native_reason()


def test_version_mismatch_falls_back_to_python(monkeypatch, tmp_path: Path) -> None:
    binary = tmp_path / "fastapi-doctor-native"
    _write_executable(
        binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf '9.9.9\\n'\n  exit 0\nfi\nexit 1\n",
    )

    native_core_module._VERSION_OK_CACHE.clear()
    monkeypatch.setenv("FASTAPI_DOCTOR_NATIVE_BINARY", str(binary))
    monkeypatch.setattr(native_core_module, "_package_version", lambda: "1.2.3")

    assert native_core_module._resolve_binary() is None
    assert native_core_module.last_native_reason() == "native override version mismatch"


def test_bundled_binary_lookup_uses_packaged_asset(monkeypatch, tmp_path: Path) -> None:
    package_root = tmp_path / "fastapi_doctor"
    binary = package_root / "bin" / "linux-x86_64" / "fastapi-doctor-native"
    _write_executable(
        binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf '2.0.0\\n'\n  exit 0\nfi\nexit 1\n",
    )

    native_core_module._VERSION_OK_CACHE.clear()
    monkeypatch.delenv("FASTAPI_DOCTOR_NATIVE_BINARY", raising=False)
    monkeypatch.setattr(native_core_module.resources, "files", lambda _: package_root)
    monkeypatch.setattr(native_core_module.sys, "platform", "linux")
    monkeypatch.setattr(native_core_module.platform, "machine", lambda: "x86_64")
    monkeypatch.setattr(native_core_module, "_package_version", lambda: "2.0.0")

    assert native_core_module._resolve_binary() == binary
    assert "bundled native binary" in native_core_module.last_native_reason()


def test_native_security_subset_matches_python_runner(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(
        package_dir / "bad.py",
        (
            "import hashlib\n"
            "import subprocess\n"
            "import yaml\n\n"
            "def run(data):\n"
            "    assert data\n"
            "    yaml.load(data)\n"
            "    subprocess.run(['echo', 'hi'], shell=True)\n"
            "    return hashlib.md5(data).hexdigest()\n"
        ),
    )

    runner = _reload_runner(monkeypatch, tmp_path, code_dir="pkg")
    _prefer_repo_debug_binary(monkeypatch)
    target_rules = {
        "security/assert-in-production",
        "security/subprocess-shell-true",
        "security/unsafe-yaml-load",
        "security/weak-hash-without-flag",
    }

    monkeypatch.delenv("FASTAPI_DOCTOR_DISABLE_NATIVE", raising=False)
    native_report = runner.run_python_doctor_checks(only_rules=target_rules)

    monkeypatch.setenv("FASTAPI_DOCTOR_DISABLE_NATIVE", "1")
    legacy_report = runner.run_python_doctor_checks(only_rules=target_rules)

    assert sorted(_issue_fingerprint(issue) for issue in native_report.issues) == sorted(
        _issue_fingerprint(issue) for issue in legacy_report.issues
    )
    assert native_report.rule_counts() == legacy_report.rule_counts()


def test_native_custom_subset_matches_python_runner(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(
        package_dir / "services" / "settings.py",
        (
            "import os\n\n"
            "def read_value():\n"
            "    return os.environ['TOKEN']\n"
        ),
    )
    _write(
        package_dir / "db.py",
        (
            "from sqlalchemy import create_engine\n\n"
            "engine = create_engine('sqlite://')\n"
        ),
    )
    _write(
        package_dir / "security.py",
        (
            "from fastapi import HTTPException\n"
            "from fastapi.middleware.cors import CORSMiddleware\n"
            "from datetime import datetime\n\n"
            "middleware = CORSMiddleware(app=None, allow_origins=['*'])\n"
            "value = datetime.utcnow()\n"
            "error = HTTPException(status_code=500, detail=str(exc))\n"
        ),
    )

    runner = _reload_runner(monkeypatch, tmp_path, code_dir="pkg")
    _prefer_repo_debug_binary(monkeypatch)
    target_rules = {
        "config/direct-env-access",
        "correctness/naive-datetime",
        "security/cors-wildcard",
        "security/exception-detail-leak",
        "resilience/sqlalchemy-pool-pre-ping",
    }

    monkeypatch.delenv("FASTAPI_DOCTOR_DISABLE_NATIVE", raising=False)
    native_report = runner.run_python_doctor_checks(only_rules=target_rules)

    monkeypatch.setenv("FASTAPI_DOCTOR_DISABLE_NATIVE", "1")
    legacy_report = runner.run_python_doctor_checks(only_rules=target_rules)

    assert sorted(_issue_fingerprint(issue) for issue in native_report.issues) == sorted(
        _issue_fingerprint(issue) for issue in legacy_report.issues
    )
    assert native_report.rule_counts() == legacy_report.rule_counts()


def test_native_fallback_subset_matches_python_runner(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(
        package_dir / "module.py",
        (
            "from typing import List, Optional\n"
            "from somewhere import *\n"
            "import os\n"
            "import openai\n"
            "import alpha\n"
            "import beta\n"
            "import gamma\n"
            "import delta\n"
            "import epsilon\n"
            "import zeta\n"
            "import eta\n"
            "import theta\n"
            "import iota\n"
            "import kappa\n"
            "import lambda_mod\n"
            "import mu\n"
            "import nu\n"
            "import xi\n"
            "import omicron\n"
            "import pi\n"
            "import rho\n"
            "import sigma\n"
            "import tau\n"
            "import upsilon\n"
            "import phi\n"
            "import chi\n"
            "import psi\n"
            "import omega\n"
            "import one\n"
            "import two\n"
            "import three\n"
            "import four\n"
            "import five\n"
            "def run() -> None:\n"
            "    print('hello')\n"
            "    value = os.path.join('a', 'b')\n"
            "    return value\n"
        ),
    )

    runner = _reload_runner(monkeypatch, tmp_path, code_dir="pkg")
    _prefer_repo_debug_binary(monkeypatch)
    target_rules = {
        "architecture/import-bloat",
        "architecture/print-in-production",
        "architecture/star-import",
        "correctness/avoid-os-path",
        "correctness/deprecated-typing-imports",
        "performance/heavy-imports",
    }

    monkeypatch.delenv("FASTAPI_DOCTOR_DISABLE_NATIVE", raising=False)
    native_report = runner.run_python_doctor_checks(only_rules=target_rules)

    monkeypatch.setenv("FASTAPI_DOCTOR_DISABLE_NATIVE", "1")
    legacy_report = runner.run_python_doctor_checks(only_rules=target_rules)

    assert sorted(_issue_fingerprint(issue) for issue in native_report.issues) == sorted(
        _issue_fingerprint(issue) for issue in legacy_report.issues
    )
    assert native_report.rule_counts() == legacy_report.rule_counts()


def test_native_static_correctness_subset_matches_python_runner(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(
        package_dir / "module.py",
        (
            "import asyncio\n"
            "import threading\n\n"
            "async def worker(items=[]):\n"
            "    lock = threading.Lock()\n"
            "    return lock\n\n"
            "asyncio.run(worker())\n\n"
            "def build():\n"
            "    try:\n"
            "        return 1\n"
            "    finally:\n"
            "        return 2\n\n"
            "def dead():\n"
            "    return 1\n"
            "    value = 2\n"
        ),
    )

    runner = _reload_runner(monkeypatch, tmp_path, code_dir="pkg")
    _prefer_repo_debug_binary(monkeypatch)
    target_rules = {
        "correctness/asyncio-run-in-async",
        "correctness/mutable-default-arg",
        "correctness/return-in-finally",
        "correctness/threading-lock-in-async",
        "correctness/unreachable-code",
    }

    monkeypatch.delenv("FASTAPI_DOCTOR_DISABLE_NATIVE", raising=False)
    native_report = runner.run_python_doctor_checks(only_rules=target_rules)

    monkeypatch.setenv("FASTAPI_DOCTOR_DISABLE_NATIVE", "1")
    legacy_report = runner.run_python_doctor_checks(only_rules=target_rules)

    assert sorted(_issue_fingerprint(issue) for issue in native_report.issues) == sorted(
        _issue_fingerprint(issue) for issue in legacy_report.issues
    )
    assert native_report.rule_counts() == legacy_report.rule_counts()
