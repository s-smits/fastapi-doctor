from __future__ import annotations

from pathlib import Path

import pytest

import fastapi_doctor.native_core as native_core  # noqa: E402


def _write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)


def test_native_rule_ids_expose_the_rust_registry() -> None:
    rule_ids = native_core.get_native_rule_ids()
    assert len(rule_ids) > 40
    assert "security/unsafe-yaml-load" in rule_ids
    assert "correctness/naive-datetime" in rule_ids
    assert "architecture/giant-function" in rule_ids


def test_native_project_scan_returns_doctor_issues(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(package_dir / "bad.py", "import yaml\nyaml.load('x')\n")

    monkeypatch.chdir(tmp_path)
    monkeypatch.setenv("DOCTOR_REPO_ROOT", str(tmp_path))
    monkeypatch.setenv("DOCTOR_CODE_DIR", "pkg")
    monkeypatch.setenv("DOCTOR_IMPORT_ROOT", str(tmp_path))

    result = native_core.analyze_selected_current_project_v2(
        profile="security",
        only_rules=["security/unsafe-yaml-load"],
        ignore_rules=None,
        skip_structure=False,
        skip_openapi=False,
        static_only=True,
        include_routes=False,
    )

    checks = {issue["check"] for issue in result["issues"]}
    assert "security/unsafe-yaml-load" in checks
    assert result["score"] <= 100
