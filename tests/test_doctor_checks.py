from __future__ import annotations

import importlib
import sys
from pathlib import Path
from types import SimpleNamespace


ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src"
if str(SRC) not in sys.path:
    sys.path.insert(0, str(SRC))

import fastapi_doctor.app_loader as app_loader_module  # noqa: E402
import fastapi_doctor.cli as cli_module  # noqa: E402
import fastapi_doctor.checks.static_checks as static_checks_module  # noqa: E402
import fastapi_doctor.models as models_module  # noqa: E402
import fastapi_doctor.project as project_module  # noqa: E402
import fastapi_doctor.runner as runner_module  # noqa: E402


def _write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)


def _reload_doctor(
    monkeypatch,
    repo_root: Path,
    *,
    code_dir: str | None = None,
    import_root: str | None = None,
    app_module: str | None = None,
):
    monkeypatch.setenv("DOCTOR_REPO_ROOT", str(repo_root))
    if code_dir is None:
        monkeypatch.delenv("DOCTOR_CODE_DIR", raising=False)
    else:
        monkeypatch.setenv("DOCTOR_CODE_DIR", code_dir)
    if import_root is None:
        monkeypatch.delenv("DOCTOR_IMPORT_ROOT", raising=False)
    else:
        monkeypatch.setenv("DOCTOR_IMPORT_ROOT", import_root)
    if app_module is None:
        monkeypatch.delenv("DOCTOR_APP_MODULE", raising=False)
    else:
        monkeypatch.setenv("DOCTOR_APP_MODULE", app_module)
    project = importlib.reload(project_module)
    app_loader = importlib.reload(app_loader_module)
    static_checks = importlib.reload(static_checks_module)
    runner = importlib.reload(runner_module)
    return SimpleNamespace(
        get_project_layout=project.get_project_layout,
        build_app_for_doctor=app_loader.build_app_for_doctor,
        run_python_doctor_checks=runner.run_python_doctor_checks,
        check_get_with_side_effect=static_checks.check_get_with_side_effect,
        check_n_plus_one_hint=static_checks.check_n_plus_one_hint,
    )


def test_auto_detects_src_layout(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "src" / "custom_backend"
    _write(package_dir / "__init__.py", "")
    _write(
        package_dir / "api.py",
        "from fastapi import FastAPI\n\napp = FastAPI(title='custom')\n",
    )

    module = _reload_doctor(monkeypatch, tmp_path)
    layout = module.get_project_layout()

    assert layout.import_root == tmp_path / "src"
    assert layout.code_dir == package_dir
    assert layout.app_module == "custom_backend.api:app"


def test_auto_detects_app_factory(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "backend" / "service"
    _write(package_dir / "__init__.py", "")
    _write(
        package_dir / "app.py",
        (
            "from fastapi import FastAPI\n\n"
            "def create_app() -> FastAPI:\n"
            "    return FastAPI(title='factory-app')\n"
        ),
    )

    module = _reload_doctor(monkeypatch, tmp_path)
    layout = module.get_project_layout()

    assert layout.import_root == tmp_path / "backend"
    assert layout.code_dir == package_dir
    assert layout.app_module == "service.app:create_app()"
    assert module.build_app_for_doctor().title == "factory-app"


def test_auto_detection_ignores_hidden_reference_dirs(monkeypatch, tmp_path: Path) -> None:
    hidden_dir = tmp_path / ".reference" / "refapp"
    _write(hidden_dir / "__init__.py", "")
    _write(hidden_dir / "api.py", "from fastapi import FastAPI\n\napp = FastAPI(title='hidden')\n")

    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(package_dir / "main.py", "from fastapi import FastAPI\n\napp = FastAPI(title='visible')\n")

    module = _reload_doctor(monkeypatch, tmp_path)
    layout = module.get_project_layout()

    assert layout.code_dir == package_dir
    assert layout.app_module == "pkg.main:app"


def test_only_rules_accepts_prefixes(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(package_dir / "main.py", "from fastapi import FastAPI\n\napp = FastAPI()\n")
    _write(
        package_dir / "bad.py",
        (
            "from typing import List\n\n"
            "def loud(items: List[int]) -> list[int]:\n"
            "    print(items)\n"
            "    return items\n"
        ),
    )

    module = _reload_doctor(monkeypatch, tmp_path)
    report = module.run_python_doctor_checks(only_rules={"correctness/"})

    assert report.issues
    assert all(issue.check.startswith("correctness/") for issue in report.issues)
    assert any(issue.check == "correctness/deprecated-typing-imports" for issue in report.issues)


def test_get_with_side_effect_ignores_read_only_execute(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(
        package_dir / "routers" / "trends.py",
        (
            "from fastapi import APIRouter\n"
            "from sqlalchemy import text\n\n"
            "router = APIRouter()\n\n"
            "@router.get('/history')\n"
            "async def get_history(session):\n"
            "    await session.execute(text('SELECT * FROM trend_runs'))\n"
            "    return {'ok': True}\n"
        ),
    )

    module = _reload_doctor(monkeypatch, tmp_path, code_dir="pkg")
    assert module.check_get_with_side_effect() == []


def test_n_plus_one_requires_loop_data_flow(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(
        package_dir / "service.py",
        (
            "from sqlalchemy import text\n\n"
            "async def load_users(session, ids, User):\n"
            "    for attempt in range(3):\n"
            "        await session.execute(text('SELECT 1'))\n"
            "    for user_id in ids:\n"
            "        await session.get(User, user_id)\n"
        ),
    )

    module = _reload_doctor(monkeypatch, tmp_path, code_dir="pkg")
    issues = module.check_n_plus_one_hint()

    assert len(issues) == 1
    assert issues[0].check == "performance/n-plus-one-hint"
    assert issues[0].line == 7


def test_issue_dict_exposes_agent_fields() -> None:
    issue = models_module.DoctorIssue(
        check="security/missing-auth-dep",
        severity="error",
        message="Protected route is missing auth dependency",
        path="app/api/routes/users.py",
        category="Security",
        help="Add a dependency like Depends(require_auth) to the route.",
        line=12,
    )

    payload = issue.to_dict()

    assert payload["blocking"] is True
    assert payload["priority"] == "high"
    assert payload["safe_to_autofix"] is False
    assert "why_it_matters" in payload
    assert payload["suggested_fix"] == "Add a dependency like Depends(require_auth) to the route."
    assert payload["fingerprint"] == "security/missing-auth-dep:app/api/routes/users.py:12:0"


def test_report_dict_includes_next_actions() -> None:
    report = models_module.DoctorReport(
        route_count=2,
        openapi_path_count=2,
        issues=[
            models_module.DoctorIssue(
                check="security/missing-auth-dep",
                severity="error",
                message="Protected route is missing auth dependency",
                path="app/api/routes/users.py",
                category="Security",
                help="Add a dependency like Depends(require_auth) to the route.",
                line=12,
            ),
            models_module.DoctorIssue(
                check="architecture/print-in-production",
                severity="warning",
                message="Use logger instead of print()",
                path="app/service.py",
                category="Architecture",
                help="Replace print() calls with a structured logger.",
                line=8,
            ),
        ],
    )

    payload = report.to_dict()

    assert payload["schema_version"] == models_module.SCHEMA_VERSION
    assert payload["rule_counts"]["security/missing-auth-dep"] == 1
    assert payload["next_actions"][0]["rule"] == "security/missing-auth-dep"
    assert payload["next_actions"][0]["blocking"] is True


def test_build_json_payload_includes_effective_config(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(package_dir / "main.py", "from fastapi import FastAPI\n\napp = FastAPI()\n")

    _reload_doctor(monkeypatch, tmp_path)
    report = models_module.DoctorReport(route_count=1, openapi_path_count=1, issues=[])
    args = SimpleNamespace(
        repo_root=None,
        code_dir=None,
        import_root=None,
        app_module=None,
        only_rules=None,
        ignore_rules=None,
        fail_on="none",
        with_bandit=False,
        with_tests=False,
        skip_ruff=True,
        skip_pyright=True,
        skip_structure=False,
        skip_openapi=False,
    )

    payload = cli_module.build_json_payload(
        args=args,
        command_results=[],
        doctor_report=report,
        final_score=report.score,
        final_label=report.label,
    )

    assert payload["schema_version"] == models_module.SCHEMA_VERSION
    assert payload["effective_config"]["architecture"]["enabled"] is True
    assert payload["project"]["app_module"] == "pkg.main:app"
