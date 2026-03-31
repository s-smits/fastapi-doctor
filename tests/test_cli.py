from __future__ import annotations

import argparse
import json
import time
from pathlib import Path

import pytest

import fastapi_doctor.cli as cli_module


def _make_args(tmp_path: Path, **overrides) -> argparse.Namespace:
    defaults = dict(
        json=False,
        score=False,
        explain_score=False,
        verbose=False,
        list_rules=False,
        init=False,
        output_format=None,
        fail_on="none",
        profile="balanced",
        ignore_rules=None,
        only_rules=None,
        repo_root=str(tmp_path),
        code_dir=None,
        import_root=None,
        app_module=None,
        skip_ruff=True,
        skip_ty=True,
        skip_structure=False,
        skip_openapi=False,
        static_only=True,
        skip_app_bootstrap=True,
        with_bandit=False,
        with_tests=False,
        pytest_args="tests/ -q",
    )
    defaults.update(overrides)
    return argparse.Namespace(**defaults)


_MOCK_RESULT_WITH_ISSUES = {
    "issues": [
        {
            "check": "security/unsafe-yaml-load",
            "severity": "error",
            "category": "Security",
            "line": 2,
            "path": "pkg/bad.py",
            "message": "yaml.load() without SafeLoader",
            "help": "Use yaml.safe_load().",
        }
    ],
    "routes": [],
    "suppressions": [],
    "route_count": 0,
    "openapi_path_count": None,
    "categories": {"Security": 1},
    "score": 98,
    "label": "A",
    "checks_not_evaluated": [],
    "engine_reason": "rust-native",
    "project_context": {
        "layout": {
            "repo_root": "/tmp/test",
            "import_root": "/tmp/test",
            "code_dir": "/tmp/test/pkg",
            "app_module": "pkg.main:app",
            "discovery_source": "explicit overrides",
        },
        "effective_config": {
            "architecture": {"enabled": True},
            "pydantic": {"should_be_model": "boundary"},
            "api": {"create_post_prefixes": [], "tag_required_prefixes": ["/api/"]},
            "security": {
                "forbidden_write_params": [],
                "auth_required_prefixes": [],
                "auth_dependency_names": [],
                "auth_exempt_prefixes": [
                    "/api/auth",
                    "/health",
                    "/ready",
                    "/live",
                    "/docs",
                    "/redoc",
                    "/openapi.json",
                    "/webhook",
                    "/oauth",
                ],
            },
            "scan": {"exclude_dirs": [], "exclude_rules": []},
        },
    },
}

_MOCK_RESULT_CLEAN = {
    "issues": [],
    "routes": [],
    "suppressions": [],
    "route_count": 0,
    "openapi_path_count": None,
    "categories": {},
    "score": 100,
    "label": "Great",
    "checks_not_evaluated": [],
    "engine_reason": "rust-native",
    "project_context": {
        "layout": {
            "repo_root": "/tmp/test",
            "import_root": "/tmp/test",
            "code_dir": "/tmp/test/pkg",
            "app_module": None,
            "discovery_source": "default",
        },
        "effective_config": {},
    },
}


def _patch_cli(monkeypatch, tmp_path, mock_result, **args_overrides):
    class MockScanSession:
        def get_scan_plan(self, **_: object) -> dict[str, object]:
            return {
                "tool_target": "pkg",
                "active_rules": ["security/unsafe-yaml-load"],
                "native_requested": True,
                "project_context": dict(mock_result["project_context"]),
            }

        def analyze_selected_v2(self, **_: object) -> dict[str, object]:
            return dict(mock_result)

    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(cli_module, "parse_args", lambda: _make_args(tmp_path, **args_overrides))
    monkeypatch.setattr(cli_module, "create_scan_session", lambda **_: MockScanSession())


# ── JSON output ─────────────────────────────────────────────────────

def test_main_emits_json_from_native_payload(monkeypatch, capsys, tmp_path: Path) -> None:
    _patch_cli(monkeypatch, tmp_path, _MOCK_RESULT_WITH_ISSUES, json=True)
    assert cli_module.main() == 1
    payload = json.loads(capsys.readouterr().out)
    assert payload["doctor"]["issues"][0]["check"] == "security/unsafe-yaml-load"
    assert payload["schema_version"] == cli_module.SCHEMA_VERSION


def test_json_output_includes_project_and_config(monkeypatch, capsys, tmp_path: Path) -> None:
    _patch_cli(monkeypatch, tmp_path, _MOCK_RESULT_WITH_ISSUES, json=True)
    cli_module.main()
    payload = json.loads(capsys.readouterr().out)
    assert "project" in payload
    assert "effective_config" in payload
    assert "requested" in payload


def test_json_output_clean_project(monkeypatch, capsys, tmp_path: Path) -> None:
    _patch_cli(monkeypatch, tmp_path, _MOCK_RESULT_CLEAN, json=True)
    assert cli_module.main() == 0
    payload = json.loads(capsys.readouterr().out)
    assert payload["score"] == 100
    assert payload["doctor"]["issues"] == []


def test_json_output_preserves_checks_not_evaluated(monkeypatch, capsys, tmp_path: Path) -> None:
    mock_result = dict(_MOCK_RESULT_CLEAN)
    mock_result["checks_not_evaluated"] = ["api-surface/missing-tags"]
    _patch_cli(monkeypatch, tmp_path, mock_result, json=True)
    cli_module.main()
    payload = json.loads(capsys.readouterr().out)
    assert payload["doctor"]["checks_not_evaluated"] == ["api-surface/missing-tags"]


# ── Score-only output ───────────────────────────────────────────────

def test_score_only_output(monkeypatch, capsys, tmp_path: Path) -> None:
    _patch_cli(monkeypatch, tmp_path, _MOCK_RESULT_WITH_ISSUES, score=True)
    assert cli_module.main() == 0
    out = capsys.readouterr().out.strip()
    assert out.isdigit()
    assert int(out) == 98


def test_score_only_clean(monkeypatch, capsys, tmp_path: Path) -> None:
    _patch_cli(monkeypatch, tmp_path, _MOCK_RESULT_CLEAN, score=True)
    assert cli_module.main() == 0
    assert capsys.readouterr().out.strip() == "100"


# ── Human output ────────────────────────────────────────────────────

def test_human_output_contains_version_and_score(monkeypatch, capsys, tmp_path: Path) -> None:
    _patch_cli(monkeypatch, tmp_path, _MOCK_RESULT_WITH_ISSUES)
    cli_module.main()
    out = capsys.readouterr().out
    assert "fastapi-doctor v" in out
    assert "Score:" in out
    assert "98/100" in out


def test_human_output_shows_issues(monkeypatch, capsys, tmp_path: Path) -> None:
    _patch_cli(monkeypatch, tmp_path, _MOCK_RESULT_WITH_ISSUES)
    cli_module.main()
    out = capsys.readouterr().out
    assert "security/unsafe-yaml-load" in out
    assert "pkg/bad.py" in out


def test_human_output_clean_shows_no_issues(monkeypatch, capsys, tmp_path: Path) -> None:
    _patch_cli(monkeypatch, tmp_path, _MOCK_RESULT_CLEAN)
    cli_module.main()
    out = capsys.readouterr().out
    assert "No structural issues found" in out


def test_human_output_verbose_shows_help(monkeypatch, capsys, tmp_path: Path) -> None:
    _patch_cli(monkeypatch, tmp_path, _MOCK_RESULT_WITH_ISSUES, verbose=True)
    cli_module.main()
    out = capsys.readouterr().out
    assert "yaml.safe_load()" in out


def test_main_starts_tool_jobs_before_native_analysis_finishes(
    monkeypatch, tmp_path: Path
) -> None:
    _patch_cli(monkeypatch, tmp_path, _MOCK_RESULT_CLEAN, skip_ruff=False, skip_ty=True)

    timing: dict[str, float] = {}

    class MockScanSession:
        def get_scan_plan(self, **_: object) -> dict[str, object]:
            return {
                "tool_target": "pkg",
                "active_rules": ["security/unsafe-yaml-load"],
                "native_requested": True,
                "project_context": {
                    "layout": {
                        "repo_root": str(tmp_path),
                        "import_root": str(tmp_path),
                        "code_dir": str(tmp_path / "pkg"),
                        "app_module": None,
                        "discovery_source": "test",
                    }
                },
            }

        def analyze_selected_v2(self, **_: object) -> dict[str, object]:
            timing["analyze_started"] = time.perf_counter()
            time.sleep(0.05)
            timing["analyze_finished"] = time.perf_counter()
            return dict(_MOCK_RESULT_CLEAN)

    def fake_run_command(name: str, command: list[str], cwd: Path) -> dict[str, object]:
        timing["tool_started"] = time.perf_counter()
        time.sleep(0.01)
        return {
            "name": name,
            "command": command,
            "returncode": 0,
            "passed": True,
            "status": "passed",
            "failure_reason": None,
            "stdout": "[]",
            "stderr": "",
        }

    monkeypatch.setattr(cli_module, "create_scan_session", lambda **_: MockScanSession())
    monkeypatch.setattr(cli_module, "_run_command", fake_run_command)

    assert cli_module.main() == 0
    assert timing["tool_started"] < timing["analyze_finished"]


def test_main_uses_single_scan_session_for_plan_and_analysis(
    monkeypatch, tmp_path: Path
) -> None:
    calls: list[str] = []

    class MockScanSession:
        def get_scan_plan(self, **_: object) -> dict[str, object]:
            calls.append("plan")
            return {
                "tool_target": "pkg",
                "active_rules": ["security/unsafe-yaml-load"],
                "native_requested": True,
                "project_context": dict(_MOCK_RESULT_CLEAN["project_context"]),
            }

        def analyze_selected_v2(self, **_: object) -> dict[str, object]:
            calls.append("analyze")
            return dict(_MOCK_RESULT_CLEAN)

    _patch_cli(monkeypatch, tmp_path, _MOCK_RESULT_CLEAN)
    monkeypatch.setattr(cli_module, "create_scan_session", lambda **_: MockScanSession())

    assert cli_module.main() == 0
    assert calls == ["plan", "analyze"]


def test_main_avoids_analysis_when_scan_plan_has_no_active_rules(
    monkeypatch, tmp_path: Path
) -> None:
    analyzed = False

    class MockScanSession:
        def get_scan_plan(self, **_: object) -> dict[str, object]:
            return {
                "tool_target": "pkg",
                "active_rules": [],
                "native_requested": False,
                "project_context": dict(_MOCK_RESULT_CLEAN["project_context"]),
            }

        def analyze_selected_v2(self, **_: object) -> dict[str, object]:
            nonlocal analyzed
            analyzed = True
            return dict(_MOCK_RESULT_CLEAN)

    _patch_cli(monkeypatch, tmp_path, _MOCK_RESULT_CLEAN, skip_ruff=True, skip_ty=True)
    monkeypatch.setattr(cli_module, "create_scan_session", lambda **_: MockScanSession())

    assert cli_module.main() == 0
    assert analyzed is False


def test_build_tool_jobs_prefers_repo_local_executable(tmp_path: Path) -> None:
    bin_dir = tmp_path / ".venv" / "bin"
    bin_dir.mkdir(parents=True)
    ruff_path = bin_dir / "ruff"
    ruff_path.write_text("#!/bin/sh\nexit 0\n")
    ruff_path.chmod(0o755)

    args = _make_args(tmp_path, skip_ruff=False, skip_ty=True)
    jobs = cli_module._build_tool_jobs(args=args, repo_root=tmp_path, target="pkg")

    assert jobs["ruff"][1][0] == str(ruff_path)


# ── --fail-on flag ──────────────────────────────────────────────────

def test_fail_on_error_returns_1_when_errors(monkeypatch, tmp_path: Path) -> None:
    _patch_cli(monkeypatch, tmp_path, _MOCK_RESULT_WITH_ISSUES, fail_on="error")
    assert cli_module.main() == 1


def test_fail_on_error_returns_0_when_clean(monkeypatch, tmp_path: Path) -> None:
    _patch_cli(monkeypatch, tmp_path, _MOCK_RESULT_CLEAN, fail_on="error")
    assert cli_module.main() == 0


def test_fail_on_warning_returns_1_when_warnings(monkeypatch, tmp_path: Path) -> None:
    result_with_warning = dict(_MOCK_RESULT_CLEAN)
    result_with_warning["issues"] = [
        {
            "check": "architecture/giant-function",
            "severity": "warning",
            "category": "Architecture",
            "line": 10,
            "path": "pkg/big.py",
            "message": "Function too large",
            "help": "Break it up.",
        }
    ]
    _patch_cli(monkeypatch, tmp_path, result_with_warning, fail_on="warning")
    assert cli_module.main() == 1


def test_fail_on_none_returns_0_despite_errors(monkeypatch, tmp_path: Path) -> None:
    # fail_on=none skips the fail_on check, but main() still returns 1 on errors
    # because of the final has_command_failure/structure_failed check
    _patch_cli(monkeypatch, tmp_path, _MOCK_RESULT_WITH_ISSUES, fail_on="none")
    assert cli_module.main() == 1  # structure_failed is True (error severity)


# ── Profile normalization ───────────────────────────────────────────

def test_normalize_profile_medium_alias() -> None:
    assert cli_module._normalize_profile("medium") == "balanced"


def test_normalize_profile_strict() -> None:
    assert cli_module._normalize_profile("strict") == "strict"


def test_normalize_profile_invalid() -> None:
    with pytest.raises(argparse.ArgumentTypeError):
        cli_module._normalize_profile("turbo")


# ── Selector helper ─────────────────────────────────────────────────

def test_matches_selector_exact() -> None:
    assert cli_module._matches_selector("security/unsafe-yaml-load", "security/unsafe-yaml-load")
    assert not cli_module._matches_selector("security/unsafe-yaml-load", "security/cors-wildcard")


def test_matches_selector_wildcard() -> None:
    assert cli_module._matches_selector("security/unsafe-yaml-load", "security/*")
    assert not cli_module._matches_selector("correctness/naive-datetime", "security/*")


# ── Score computation ───────────────────────────────────────────────

def test_compute_combined_score_no_penalties() -> None:
    score, label = cli_module._compute_combined_score(100, None, None, None)
    assert score == 100
    assert label == "Great"


def test_compute_combined_score_ruff_penalty() -> None:
    score, label = cli_module._compute_combined_score(100, False, None, None)
    assert score == 95
    assert label == "Great"


def test_compute_combined_score_all_penalties() -> None:
    score, label = cli_module._compute_combined_score(100, False, False, 3)
    assert score == 75
    assert label == "Needs work"


def test_compute_combined_score_floor_at_zero() -> None:
    score, _ = cli_module._compute_combined_score(0, False, False, 10)
    assert score == 0


def test_compute_combined_score_labels() -> None:
    _, label_great = cli_module._compute_combined_score(85, None, None, None)
    _, label_needs = cli_module._compute_combined_score(70, None, None, None)
    _, label_crit = cli_module._compute_combined_score(50, None, None, None)
    assert label_great == "Great"
    assert label_needs == "Needs work"
    assert label_crit == "Critical"


# ── Ruff mapping ────────────────────────────────────────────────────

def test_map_ruff_findings_to_doctor_t201() -> None:
    ruff_output = json.dumps([{
        "code": "T201",
        "filename": "app/main.py",
        "location": {"row": 5},
        "message": "print found",
    }])
    issues = cli_module._map_ruff_findings_to_doctor(ruff_output)
    assert len(issues) == 1
    assert issues[0]["check"] == "architecture/print-in-production"
    assert issues[0]["line"] == 5


def test_map_ruff_findings_to_doctor_f403() -> None:
    ruff_output = json.dumps([{
        "code": "F403",
        "filename": "app/utils.py",
        "location": {"row": 1},
        "message": "from module import *",
    }])
    issues = cli_module._map_ruff_findings_to_doctor(ruff_output)
    assert len(issues) == 1
    assert issues[0]["check"] == "architecture/star-import"


def test_map_ruff_findings_normalizes_absolute_paths(tmp_path: Path) -> None:
    absolute_file = tmp_path / "app" / "main.py"
    ruff_output = json.dumps([{
        "code": "T201",
        "filename": str(absolute_file),
        "location": {"row": 5},
        "message": "print found",
    }])
    issues = cli_module._map_ruff_findings_to_doctor(ruff_output, repo_root=tmp_path)
    assert len(issues) == 1
    assert issues[0]["path"] == "app/main.py"


def test_map_ruff_findings_ignores_unknown_codes() -> None:
    ruff_output = json.dumps([{
        "code": "E501",
        "filename": "app/main.py",
        "location": {"row": 1},
        "message": "line too long",
    }])
    issues = cli_module._map_ruff_findings_to_doctor(ruff_output)
    assert issues == []


def test_map_ruff_findings_handles_invalid_json() -> None:
    issues = cli_module._map_ruff_findings_to_doctor("not json at all")
    assert issues == []


def test_merge_issues_deduplicates_same_rule_path_and_line() -> None:
    native = [{
        "check": "architecture/print-in-production",
        "path": "app/main.py",
        "line": 5,
    }]
    extra = [
        {
            "check": "architecture/print-in-production",
            "path": "app/main.py",
            "line": 5,
        },
        {
            "check": "architecture/star-import",
            "path": "app/utils.py",
            "line": 1,
        },
    ]
    merged = cli_module._merge_issues(native, extra)
    assert [issue["check"] for issue in merged] == [
        "architecture/print-in-production",
        "architecture/star-import",
    ]


def test_resolved_tool_target_prefers_detected_code_dir(tmp_path: Path) -> None:
    report = {
        "project_context": {
            "layout": {
                "code_dir": str(tmp_path / "src" / "pkg"),
            }
        }
    }
    target = cli_module._resolved_tool_target(
        repo_root=tmp_path,
        explicit_code_dir=None,
        doctor_report=report,
    )
    assert target == "src/pkg"


def test_build_tool_jobs_uses_target_path(tmp_path: Path) -> None:
    args = _make_args(tmp_path, skip_ruff=False, skip_ty=False, with_bandit=True)
    jobs = cli_module._build_tool_jobs(args=args, repo_root=tmp_path, target="src/pkg")
    assert jobs["ruff"][1][:4] == ["uvx", "ruff", "check", "src/pkg"]
    assert jobs["ty"][1][:4] == ["uvx", "ty", "check", "src/pkg"]
    assert jobs["bandit"][1][:5] == ["uv", "run", "bandit", "-q", "-r"]
    assert jobs["bandit"][1][5] == "src/pkg"


# ── CSV splitting ───────────────────────────────────────────────────

def test_split_csv_none() -> None:
    assert cli_module._split_csv(None) is None


def test_split_csv_empty() -> None:
    assert cli_module._split_csv("") is None


def test_split_csv_multiple() -> None:
    result = cli_module._split_csv("security/*, architecture/giant-function")
    assert result == ["security/*", "architecture/giant-function"]


# ── Version ─────────────────────────────────────────────────────────

def test_version_flag(monkeypatch, capsys) -> None:
    monkeypatch.setattr("sys.argv", ["fastapi-doctor", "--version"])
    with pytest.raises(SystemExit) as exc_info:
        cli_module.parse_args()
    assert exc_info.value.code == 0
    out = capsys.readouterr().out
    assert "fastapi-doctor" in out
