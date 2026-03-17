from __future__ import annotations

import importlib
import sys
from pathlib import Path
from types import SimpleNamespace


ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src"
if str(SRC) not in sys.path:
    sys.path.insert(0, str(SRC))

import python_doctor.app_loader as app_loader_module  # noqa: E402
import python_doctor.checks.static_checks as static_checks_module  # noqa: E402
import python_doctor.project as project_module  # noqa: E402
import python_doctor.runner as runner_module  # noqa: E402


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
