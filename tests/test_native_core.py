from __future__ import annotations

import importlib
import sys
from pathlib import Path

import pytest


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


def test_native_security_subset(monkeypatch, tmp_path: Path) -> None:
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
    target_rules = {
        "security/assert-in-production",
        "security/subprocess-shell-true",
        "security/unsafe-yaml-load",
        "security/weak-hash-without-flag",
    }

    monkeypatch.delenv("FASTAPI_DOCTOR_DISABLE_NATIVE", raising=False)
    report = runner.run_python_doctor_checks(only_rules=target_rules)

    found = {issue.check for issue in report.issues}
    assert "security/assert-in-production" in found
    assert "security/subprocess-shell-true" in found
    assert "security/unsafe-yaml-load" in found
    assert "security/weak-hash-without-flag" in found


def test_native_custom_subset(monkeypatch, tmp_path: Path) -> None:
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
    target_rules = {
        "config/direct-env-access",
        "correctness/naive-datetime",
        "security/cors-wildcard",
        "security/exception-detail-leak",
        "resilience/sqlalchemy-pool-pre-ping",
    }

    monkeypatch.delenv("FASTAPI_DOCTOR_DISABLE_NATIVE", raising=False)
    report = runner.run_python_doctor_checks(only_rules=target_rules)

    found = {issue.check for issue in report.issues}
    assert "correctness/naive-datetime" in found
    assert "security/cors-wildcard" in found
    assert "security/exception-detail-leak" in found
    assert "resilience/sqlalchemy-pool-pre-ping" in found


def test_native_architecture_and_correctness_subset(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(
        package_dir / "module.py",
        (
            "from typing import List, Optional\n"
            "from somewhere import *\n"
            "import os\n"
            "import openai\n"
            "import alpha, beta, gamma, delta, epsilon, zeta, eta, theta\n"
            "import iota, kappa, lambda_mod, mu, nu, xi, omicron, pi\n"
            "import rho, sigma, tau, upsilon, phi, chi, psi, omega\n"
            "import one, two, three, four, five\n"
            "def run() -> None:\n"
            "    print('hello')\n"
            "    value = os.path.join('a', 'b')\n"
            "    return value\n"
        ),
    )

    runner = _reload_runner(monkeypatch, tmp_path, code_dir="pkg")
    target_rules = {
        "architecture/import-bloat",
        "architecture/print-in-production",
        "architecture/star-import",
        "correctness/avoid-os-path",
        "correctness/deprecated-typing-imports",
        "performance/heavy-imports",
    }

    monkeypatch.delenv("FASTAPI_DOCTOR_DISABLE_NATIVE", raising=False)
    report = runner.run_python_doctor_checks(only_rules=target_rules)

    found = {issue.check for issue in report.issues}
    assert "architecture/print-in-production" in found
    assert "architecture/star-import" in found
    assert "correctness/deprecated-typing-imports" in found


def test_native_correctness_async_subset(monkeypatch, tmp_path: Path) -> None:
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
    target_rules = {
        "correctness/asyncio-run-in-async",
        "correctness/mutable-default-arg",
        "correctness/return-in-finally",
        "correctness/threading-lock-in-async",
        "correctness/unreachable-code",
    }

    monkeypatch.delenv("FASTAPI_DOCTOR_DISABLE_NATIVE", raising=False)
    report = runner.run_python_doctor_checks(only_rules=target_rules)

    found = {issue.check for issue in report.issues}
    assert "correctness/mutable-default-arg" in found
    assert "correctness/return-in-finally" in found
    assert "correctness/unreachable-code" in found


def test_native_disabled_produces_no_static_issues(monkeypatch, tmp_path: Path) -> None:
    """When native is disabled there is no Python fallback — static issues are empty."""
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(package_dir / "bad.py", "import yaml\nyaml.load('x')\n")

    runner = _reload_runner(monkeypatch, tmp_path, code_dir="pkg")
    monkeypatch.setenv("FASTAPI_DOCTOR_DISABLE_NATIVE", "1")
    report = runner.run_python_doctor_checks(only_rules={"security/unsafe-yaml-load"})

    assert report.issues == []


def test_get_native_rule_ids_returns_registry(monkeypatch) -> None:
    monkeypatch.delenv("FASTAPI_DOCTOR_DISABLE_NATIVE", raising=False)
    rule_ids = native_core_module.get_native_rule_ids()
    assert len(rule_ids) > 40
    assert "security/unsafe-yaml-load" in rule_ids
    assert "correctness/naive-datetime" in rule_ids
    assert "architecture/giant-function" in rule_ids


def test_selected_native_static_mode_raises_when_disabled(monkeypatch) -> None:
    monkeypatch.setenv("FASTAPI_DOCTOR_DISABLE_NATIVE", "1")

    with pytest.raises(native_core_module.NativeStaticModeUnavailable):
        native_core_module.run_native_selected_project_auto_v2(
            profile="strict",
            only_rules=None,
            ignore_rules=None,
            skip_structure=False,
            skip_openapi=False,
            static_only=True,
            require_native=True,
        )
