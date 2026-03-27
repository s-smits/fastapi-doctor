from __future__ import annotations

import importlib
import sys
import types
from pathlib import Path
from types import SimpleNamespace


ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src"
if str(SRC) not in sys.path:
    sys.path.insert(0, str(SRC))

import fastapi_doctor.app_loader as app_loader_module  # noqa: E402
import fastapi_doctor.checks.architecture as architecture_module  # noqa: E402
import fastapi_doctor.checks.route_checks as route_checks_module  # noqa: E402
import fastapi_doctor.cli as cli_module  # noqa: E402
import fastapi_doctor.external_tools as external_tools_module  # noqa: E402
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
    architecture = importlib.reload(architecture_module)
    runner = importlib.reload(runner_module)
    return SimpleNamespace(
        get_project_layout=project.get_project_layout,
        parsed_python_modules=project.parsed_python_modules,
        build_app_for_doctor=app_loader.build_app_for_doctor,
        run_python_doctor_checks=runner.run_python_doctor_checks,
        check_get_with_side_effect=static_checks.check_get_with_side_effect,
        check_n_plus_one_hint=static_checks.check_n_plus_one_hint,
        check_passthrough_functions=architecture.check_passthrough_functions,
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

    fake_fastapi = types.ModuleType("fastapi")

    class FakeFastAPI:
        def __init__(self, title: str = "") -> None:
            self.title = title

    fake_fastapi.FastAPI = FakeFastAPI
    monkeypatch.setitem(sys.modules, "fastapi", fake_fastapi)

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


def test_skip_app_bootstrap_avoids_live_app_import(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(
        package_dir / "main.py",
        "from fastapi import FastAPI\n\napp = FastAPI()\n",
    )
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

    def _boom():
        raise AssertionError("app bootstrap should be skipped")

    monkeypatch.setattr(app_loader_module, "build_app_for_doctor", _boom)
    report = module.run_python_doctor_checks(profile="strict", skip_app_bootstrap=True)

    assert any(issue.check == "correctness/deprecated-typing-imports" for issue in report.issues)
    # Route-list checks now run via static extraction — only OpenAPI checks are skipped
    assert "security/missing-auth-dep" not in report.checks_not_evaluated
    assert "api-surface/missing-operation-id" in report.checks_not_evaluated
    assert not any(issue.check == "doctor/app-bootstrap-failed" for issue in report.issues)


def test_skip_app_bootstrap_skips_app_discovery(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(package_dir / "service.py", "def compute(values=[]):\n    return values\n")

    module = _reload_doctor(monkeypatch, tmp_path)

    def _boom(repo_root: Path):  # type: ignore[unused-argument]
        raise AssertionError("static-only execution should not scan the repo for a FastAPI app candidate")

    monkeypatch.setattr(project_module, "_discover_app_candidate", _boom)

    report = module.run_python_doctor_checks(profile="strict", skip_app_bootstrap=True)

    assert report.checks_not_evaluated
    assert module.get_project_layout().discovery_source == "static-only heuristics"


def test_static_only_prefers_src_code_dir_over_repo_root(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "src" / "fastapi_doctor"
    _write(package_dir / "__init__.py", "")
    _write(package_dir / "cli.py", "def main() -> int:\n    return 0\n")
    _write(tmp_path / "tests" / "test_fixture.py", "assert True\n")

    module = _reload_doctor(monkeypatch, tmp_path)
    project_module.refresh_runtime_config(static_only=True)

    layout = module.get_project_layout()
    parsed = module.parsed_python_modules()

    assert layout.code_dir == tmp_path / "src"
    assert not any(parsed_module.rel_path.startswith("tests/") for parsed_module in parsed)


def test_missing_fastapi_runtime_skips_live_app_import(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(
        package_dir / "main.py",
        "from fastapi import FastAPI\n\napp = FastAPI()\n",
    )

    module = _reload_doctor(monkeypatch, tmp_path)

    def _boom():
        raise AssertionError("app bootstrap should be skipped when fastapi runtime is unavailable")

    monkeypatch.setattr(app_loader_module, "build_app_for_doctor", _boom)
    monkeypatch.setattr(app_loader_module, "fastapi_runtime_available", lambda: False)

    report = module.run_python_doctor_checks(profile="strict")

    assert "security/missing-auth-dep" not in report.checks_not_evaluated
    assert "api-surface/missing-operation-id" in report.checks_not_evaluated
    assert not any(issue.check == "doctor/app-bootstrap-failed" for issue in report.issues)


def test_static_native_route_policy_rules_are_reported(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(
        tmp_path / ".fastapi-doctor.yml",
        (
            "api:\n"
            "  create_post_prefixes: ['/api/items']\n"
            "security:\n"
            "  forbidden_write_params: ['user_id']\n"
        ),
    )
    _write(
        package_dir / "api.py",
        (
            "from fastapi import APIRouter\n\n"
            "router = APIRouter(prefix='/api/items')\n\n"
            "@router.post('')\n"
            "async def create_item(user_id: int):\n"
            "    return {'ok': True}\n\n"
            "@router.get('')\n"
            "async def list_items():\n"
            "    return ['a']\n"
        ),
    )

    module = _reload_doctor(monkeypatch, tmp_path, code_dir="pkg")
    report = module.run_python_doctor_checks(
        only_rules={
            "security/forbidden-write-param",
            "correctness/missing-response-model",
            "correctness/post-status-code",
            "api-surface/missing-tags",
            "api-surface/missing-docstring",
        },
        skip_app_bootstrap=True,
    )

    found = {issue.check for issue in report.issues}
    assert "security/forbidden-write-param" in found
    assert "correctness/missing-response-model" in found
    assert "correctness/post-status-code" in found
    assert "api-surface/missing-tags" in found
    assert "api-surface/missing-docstring" in found


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
    assert payload["change_scope"] == "minimal"
    assert "Avoid namespace rewrites" in payload["autofix_guidance"]
    assert payload["fingerprint"] == "security/missing-auth-dep:app/api/routes/users.py:12:0"


def test_map_ruff_findings_to_doctor() -> None:
    stdout = """
[
  {
    "code": "T201",
    "filename": "app/main.py",
    "location": {"row": 12, "column": 5},
    "message": "`print` found"
  },
  {
    "code": "F403",
    "filename": "app/utils.py",
    "location": {"row": 3, "column": 1},
    "message": "`from mod import *` used; unable to detect undefined names"
  }
]
""".strip()

    issues = external_tools_module.map_ruff_findings_to_doctor(stdout)

    assert [issue.check for issue in issues] == [
        "architecture/print-in-production",
        "architecture/star-import",
    ]
    assert issues[0].path == "app/main.py"
    assert issues[0].line == 12
    assert issues[1].path == "app/utils.py"
    assert issues[1].line == 3


def test_main_bootstraps_ruff_with_uvx(monkeypatch, tmp_path: Path, capsys) -> None:
    commands: list[list[str]] = []

    monkeypatch.setattr(
        cli_module,
        "parse_args",
        lambda: SimpleNamespace(
            static_only=False,
            json=False,
            score=True,
            skip_ruff=False,
            skip_ty=True,
            with_bandit=False,
            with_tests=False,
            skip_structure=True,
            skip_openapi=True,
            fail_on="none",
            profile="medium",
            only_rules=None,
            ignore_rules=None,
            verbose=False,
            repo_root=None,
            code_dir=None,
            import_root=None,
            app_module=None,
            skip_app_bootstrap=False,
            pytest_args="",
        ),
    )
    monkeypatch.setattr(cli_module, "configure_environment_from_args", lambda args: None)
    monkeypatch.setattr(cli_module, "resolve_repo_root", lambda: tmp_path)
    monkeypatch.setattr(cli_module, "get_cli_version", lambda: "test")

    def _fake_run_command(name: str, command: list[str], cwd: Path) -> external_tools_module.CommandResult:
        commands.append(command)
        return external_tools_module.CommandResult(
            name=name,
            command=command,
            returncode=0,
            stdout="[]",
            stderr="",
        )

    monkeypatch.setattr(cli_module, "run_command", _fake_run_command)

    assert cli_module.main() == 0
    assert commands == [["uvx", "ruff", "check", ".", "--output-format", "json"]]
    assert capsys.readouterr().out.strip() == "100"


def test_main_uses_native_static_score_fast_path(monkeypatch, capsys) -> None:
    monkeypatch.setattr(
        cli_module,
        "parse_args",
        lambda: SimpleNamespace(
            static_only=True,
            json=False,
            score=True,
            skip_ruff=True,
            skip_ty=True,
            with_bandit=False,
            with_tests=False,
            skip_structure=False,
            skip_openapi=False,
            fail_on="none",
            profile="strict",
            only_rules=None,
            ignore_rules=None,
            verbose=False,
            repo_root=None,
            code_dir=None,
            import_root=None,
            app_module=None,
            skip_app_bootstrap=False,
            pytest_args="",
        ),
    )
    monkeypatch.setattr(cli_module, "configure_environment_from_args", lambda args: None)
    monkeypatch.setattr(cli_module, "resolve_repo_root", lambda: (_ for _ in ()).throw(AssertionError("slow path should not resolve repo root")))
    monkeypatch.setattr(cli_module, "get_cli_version", lambda: (_ for _ in ()).throw(AssertionError("slow path should not load version")))

    import fastapi_doctor.native_core as native_core_module

    monkeypatch.setattr(native_core_module, "score_native_project_auto_v2", lambda **kwargs: 77)

    assert cli_module.main() == 0
    assert capsys.readouterr().out.strip() == "77"


def test_route_checks_legacy_exports_are_lazy() -> None:
    route_checks = importlib.reload(route_checks_module)

    assert route_checks.check_route_dependency_policies.__module__ == "fastapi_doctor.checks.route_checks"
    assert route_checks.check_duplicate_routes.__module__ == "fastapi_doctor.checks._legacy_route_checks"


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
        profile="strict",
        with_bandit=False,
        with_tests=False,
        skip_ruff=True,
        skip_ty=True,
        skip_structure=False,
        skip_openapi=False,
        static_only=False,
        skip_app_bootstrap=True,
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
    assert payload["requested"]["static_only"] is False
    assert payload["requested"]["skip_app_bootstrap"] is True


def test_native_project_context_includes_effective_config(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(package_dir / "main.py", "from fastapi import FastAPI\n\napp = FastAPI()\n")
    _write(
        tmp_path / ".fastapi-doctor.yml",
        (
            "architecture:\n"
            "  giant_function: 123\n"
            "pydantic:\n"
            "  should_be_model: everywhere\n"
            "api:\n"
            "  tag_required_prefixes: ['/api/', '/internal/']\n"
            "security:\n"
            "  forbidden_write_params: ['user_id']\n"
            "scan:\n"
            "  exclude_dirs: ['generated']\n"
            "  exclude_rules: ['security/*']\n"
        ),
    )

    _reload_doctor(monkeypatch, tmp_path)

    from fastapi_doctor import _fastapi_doctor_native

    payload = _fastapi_doctor_native.get_project_context(static_only=False)

    assert payload["effective_config"]["architecture"]["giant_function"] == 123
    assert payload["effective_config"]["pydantic"]["should_be_model"] == "everywhere"
    assert payload["effective_config"]["api"]["tag_required_prefixes"] == ["/api/", "/internal/"]
    assert payload["effective_config"]["security"]["forbidden_write_params"] == ["user_id"]
    assert payload["effective_config"]["scan"]["exclude_dirs"] == ["generated"]
    assert payload["effective_config"]["scan"]["exclude_rules"] == ["security/*"]


def test_refresh_runtime_config_prefers_native_effective_config(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(package_dir / "main.py", "from fastapi import FastAPI\n\napp = FastAPI()\n")
    _write(
        tmp_path / ".fastapi-doctor.yml",
        (
            "architecture:\n"
            "  giant_function: 321\n"
            "scan:\n"
            "  exclude_dirs: ['generated']\n"
        ),
    )

    _reload_doctor(monkeypatch, tmp_path)

    def _boom() -> dict[str, object]:
        raise AssertionError("Python config parsing should be bypassed when native effective config is available")

    monkeypatch.setattr(project_module, "_load_doctor_config", _boom)

    project_module.refresh_runtime_config()

    assert project_module.GIANT_FUNCTION_THRESHOLD == 321
    assert project_module.SCAN_EXCLUDED_DIRS == frozenset({"generated"})


def test_get_project_layout_refreshes_when_env_changes(monkeypatch, tmp_path: Path) -> None:
    first_repo = tmp_path / "first"
    second_repo = tmp_path / "second"
    for repo_root, title in ((first_repo, "first"), (second_repo, "second")):
        package_dir = repo_root / "pkg"
        _write(package_dir / "__init__.py", "")
        _write(package_dir / "main.py", f"from fastapi import FastAPI\n\napp = FastAPI(title='{title}')\n")

    module = _reload_doctor(monkeypatch, first_repo)
    first_layout = module.get_project_layout()

    assert first_layout.repo_root == first_repo
    assert first_layout.app_module == "pkg.main:app"

    monkeypatch.setenv("DOCTOR_REPO_ROOT", str(second_repo))
    monkeypatch.delenv("DOCTOR_CODE_DIR", raising=False)
    monkeypatch.delenv("DOCTOR_IMPORT_ROOT", raising=False)
    monkeypatch.delenv("DOCTOR_APP_MODULE", raising=False)

    second_layout = module.get_project_layout()

    assert second_layout.repo_root == second_repo
    assert second_layout.app_module == "pkg.main:app"


def test_explicit_app_module_skips_repo_scan_and_infers_layout(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "src" / "service"
    _write(package_dir / "__init__.py", "")
    _write(package_dir / "api.py", "from fastapi import FastAPI\n\ndef create_app() -> FastAPI:\n    return FastAPI()\n")

    monkeypatch.setenv("DOCTOR_REPO_ROOT", str(tmp_path))
    monkeypatch.setenv("DOCTOR_APP_MODULE", "service.api:create_app()")
    monkeypatch.delenv("DOCTOR_CODE_DIR", raising=False)
    monkeypatch.delenv("DOCTOR_IMPORT_ROOT", raising=False)

    original_discover = project_module._discover_app_candidate

    def _boom(repo_root: Path):  # type: ignore[unused-argument]
        raise AssertionError("explicit app modules should not trigger repo-wide app discovery")

    monkeypatch.setattr(project_module, "_discover_app_candidate", _boom)
    try:
        project = importlib.reload(project_module)
        layout = project.get_project_layout()
    finally:
        monkeypatch.setattr(project_module, "_discover_app_candidate", original_discover)

    assert layout.import_root == tmp_path / "src"
    assert layout.code_dir == tmp_path / "src" / "service"
    assert layout.app_module == "service.api:create_app()"


def test_parsed_python_modules_reuses_cache(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(package_dir / "main.py", "from fastapi import FastAPI\n\napp = FastAPI()\n")
    _write(package_dir / "service.py", "def compute() -> int:\n    return 1\n")

    module = _reload_doctor(monkeypatch, tmp_path)
    first = module.parsed_python_modules()
    second = module.parsed_python_modules()

    assert first is second
    assert [parsed.rel_path for parsed in first] == ["pkg/__init__.py", "pkg/main.py", "pkg/service.py"]


def test_passthrough_function_ignores_methods(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(package_dir / "main.py", "from fastapi import FastAPI\n\napp = FastAPI()\n")
    _write(
        package_dir / "service.py",
        (
            "class Delegator:\n"
            "    def method(self, a, b):\n"
            "        return target(a, b)\n\n"
            "def wrapper(a, b):\n"
            "    return target(a, b)\n\n"
            "def target(a, b):\n"
            "    return a + b\n"
        ),
    )

    module = _reload_doctor(monkeypatch, tmp_path)
    issues = module.check_passthrough_functions()

    assert len(issues) == 1
    assert issues[0].check == "architecture/passthrough-function"
    assert issues[0].line == 5


def test_alembic_target_metadata_check_requires_real_metadata(monkeypatch, tmp_path: Path) -> None:
    _write(
        tmp_path / "pyproject.toml",
        (
            "[project]\n"
            "name = 'example'\n"
            "version = '0.1.0'\n"
            "dependencies = ['sqlalchemy', 'alembic']\n"
        ),
    )
    _write(tmp_path / "app" / "__init__.py", "")
    _write(
        tmp_path / "app" / "models.py",
        "from sqlalchemy.orm import DeclarativeBase\n\nclass Base(DeclarativeBase):\n    pass\n",
    )
    _write(
        tmp_path / "alembic" / "env.py",
        (
            "from alembic import context\n\n"
            "target_metadata = None\n\n"
            "def run_migrations_online(connection):\n"
            "    context.configure(connection=connection, target_metadata=target_metadata)\n"
        ),
    )

    module = _reload_doctor(monkeypatch, tmp_path, code_dir="app")
    report = module.run_python_doctor_checks(only_rules={"config/alembic", "config/sqlalchemy-naming-convention"})

    assert [issue.check for issue in report.issues] == ["config/alembic-target-metadata"]
    assert report.issues[0].path == "alembic/env.py"


def test_alembic_best_practice_checks_flag_missing_hooks(monkeypatch, tmp_path: Path) -> None:
    _write(
        tmp_path / "pyproject.toml",
        (
            "[project]\n"
            "name = 'example'\n"
            "version = '0.1.0'\n"
            "dependencies = ['sqlalchemy', 'alembic']\n"
        ),
    )
    _write(tmp_path / "app" / "__init__.py", "")
    _write(
        tmp_path / "app" / "models.py",
        (
            "from sqlalchemy.orm import DeclarativeBase\n\n"
            "class Base(DeclarativeBase):\n"
            "    pass\n"
        ),
    )
    _write(
        tmp_path / "alembic" / "env.py",
        (
            "from alembic import context\n"
            "from app.models import Base\n\n"
            "target_metadata = Base.metadata\n\n"
            "def run_migrations_online(connection):\n"
            "    context.configure(connection=connection, target_metadata=target_metadata)\n"
        ),
    )

    module = _reload_doctor(monkeypatch, tmp_path, code_dir="app")
    report = module.run_python_doctor_checks(only_rules={"config/alembic", "config/sqlalchemy-naming-convention"})

    assert {issue.check for issue in report.issues} == {
        "config/alembic-empty-autogen-revision",
        "config/sqlalchemy-naming-convention",
    }
    assert all(issue.path == "alembic/env.py" for issue in report.issues)


def test_alembic_best_practice_checks_accept_common_hooks(monkeypatch, tmp_path: Path) -> None:
    _write(
        tmp_path / "pyproject.toml",
        (
            "[project]\n"
            "name = 'example'\n"
            "version = '0.1.0'\n"
            "dependencies = ['sqlalchemy', 'alembic']\n"
        ),
    )
    _write(tmp_path / "app" / "__init__.py", "")
    _write(
        tmp_path / "app" / "models.py",
        (
            "from sqlalchemy import MetaData\n\n"
            "NAMING_CONVENTION = {\n"
            "    'ix': 'ix_%(column_0_label)s',\n"
            "    'uq': 'uq_%(table_name)s_%(column_0_name)s',\n"
            "    'pk': 'pk_%(table_name)s',\n"
            "}\n\n"
            "metadata = MetaData(naming_convention=NAMING_CONVENTION)\n"
        ),
    )
    _write(
        tmp_path / "alembic" / "env.py",
        (
            "from alembic import context\n"
            "from app.models import metadata\n\n"
            "target_metadata = metadata\n\n"
            "def include_name(name, type_, parent_names):\n"
            "    return True\n\n"
            "def process_revision_directives(context, revision, directives):\n"
            "    if directives:\n"
            "        return\n\n"
            "def run_migrations_online(connection):\n"
            "    context.configure(\n"
            "        connection=connection,\n"
            "        target_metadata=target_metadata,\n"
            "        include_name=include_name,\n"
            "        process_revision_directives=process_revision_directives,\n"
            "    )\n"
        ),
    )

    module = _reload_doctor(monkeypatch, tmp_path, code_dir="app")
    report = module.run_python_doctor_checks(only_rules={"config/alembic", "config/sqlalchemy-naming-convention"})

    assert report.issues == []


def test_skip_app_bootstrap_requests_native_routes(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(package_dir / "main.py", "from fastapi import FastAPI\n\napp = FastAPI()\n")

    module = _reload_doctor(monkeypatch, tmp_path, code_dir="pkg")
    seen: list[bool] = []

    def _fake_native(active_rules, *, include_routes=True, static_only=True):  # type: ignore[unused-argument]
        seen.append(include_routes)
        return {
            "issues": [],
            "routes": [],
            "suppressions": [],
            "route_count": 0,
            "openapi_path_count": None,
            "categories": {},
            "score": 100,
            "label": "A",
            "checks_not_evaluated": [],
        }

    monkeypatch.setattr(runner_module.native_core, "run_native_project_auto_v2", _fake_native)

    module.run_python_doctor_checks(profile="strict", skip_app_bootstrap=True)

    assert seen == [True]


def test_static_only_without_route_rules_skips_native_routes(monkeypatch, tmp_path: Path) -> None:
    package_dir = tmp_path / "pkg"
    _write(package_dir / "__init__.py", "")
    _write(package_dir / "main.py", "from fastapi import FastAPI\n\napp = FastAPI()\n")

    module = _reload_doctor(monkeypatch, tmp_path, code_dir="pkg")
    seen: list[bool] = []

    def _fake_native(active_rules, *, include_routes=True, static_only=True):  # type: ignore[unused-argument]
        seen.append(include_routes)
        return {
            "issues": [],
            "routes": [],
            "suppressions": [],
            "route_count": 0,
            "openapi_path_count": None,
            "categories": {},
            "score": 100,
            "label": "A",
            "checks_not_evaluated": [],
        }

    monkeypatch.setattr(runner_module.native_core, "run_native_project_auto_v2", _fake_native)

    module.run_python_doctor_checks(
        only_rules={"security/assert-in-production"},
        skip_app_bootstrap=True,
    )

    assert seen == [False]
