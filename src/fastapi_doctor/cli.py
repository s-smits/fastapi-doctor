from __future__ import annotations

"""CLI entrypoint for the Rust-native fastapi-doctor package."""

import argparse
import json
import os
import shlex
import shutil
import subprocess
import sys
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from importlib.metadata import PackageNotFoundError, version as metadata_version
from pathlib import Path
from typing import Any

from .native_core import (
    NativeEngineUnavailable,
    create_scan_session,
    get_rule_metadata,
)

SCHEMA_VERSION = "1.3"


def get_cli_version() -> str:
    try:
        from ._version import version

        return version
    except ImportError:
        try:
            return metadata_version("fastapi-doctor")
        except PackageNotFoundError:
            return "0.0.0"


def _normalize_profile(value: str) -> str:
    normalized = value.strip().lower()
    if normalized == "medium":
        return "balanced"
    if normalized not in {"security", "balanced", "strict"}:
        raise argparse.ArgumentTypeError("profile must be one of: security, balanced, strict")
    return normalized


def _split_csv(value: str | None) -> list[str] | None:
    if not value:
        return None
    items = [item.strip() for item in value.split(",")]
    return [item for item in items if item]


def _matches_selector(rule_id: str, selector: str) -> bool:
    selector = selector.strip()
    if selector.endswith("*"):
        selector = selector[:-1]
    return rule_id == selector or rule_id.startswith(selector)


def _run_command(name: str, command: list[str], cwd: Path) -> dict[str, Any]:
    try:
        proc = subprocess.run(
            command,
            cwd=cwd,
            capture_output=True,
            text=True,
            check=False,
        )
    except FileNotFoundError as exc:
        return {
            "name": name,
            "command": command,
            "returncode": 127,
            "passed": False,
            "status": "command-not-found",
            "failure_reason": "command-not-found",
            "stdout": "",
            "stderr": str(exc),
        }

    return {
        "name": name,
        "command": command,
        "returncode": proc.returncode,
        "passed": proc.returncode == 0,
        "status": "passed" if proc.returncode == 0 else "failed",
        "failure_reason": None if proc.returncode == 0 else "failed",
        "stdout": proc.stdout,
        "stderr": proc.stderr,
    }


def _count_bandit_highs(stdout: str) -> int:
    try:
        payload = json.loads(stdout)
    except json.JSONDecodeError:
        return 0
    results = payload.get("results", []) if isinstance(payload, dict) else []
    return sum(
        1
        for finding in results
        if isinstance(finding, dict)
        and str(finding.get("issue_severity", "")).upper() == "HIGH"
    )


def _parse_ruff_json(stdout: str) -> list[dict[str, Any]]:
    try:
        payload = json.loads(stdout)
    except json.JSONDecodeError:
        return []
    return payload if isinstance(payload, list) else []


def _normalize_issue_path(path: str, repo_root: Path | None) -> str:
    if not repo_root:
        return path
    try:
        path_obj = Path(path)
    except OSError:
        return path
    if not path_obj.is_absolute():
        return path_obj.as_posix()
    try:
        return path_obj.relative_to(repo_root).as_posix()
    except ValueError:
        return path_obj.as_posix()


def _map_ruff_findings_to_doctor(stdout: str, *, repo_root: Path | None = None) -> list[dict[str, Any]]:
    findings = _parse_ruff_json(stdout)
    issues: list[dict[str, Any]] = []
    for finding in findings:
        if not isinstance(finding, dict):
            continue
        code = finding.get("code")
        filename = _normalize_issue_path(str(finding.get("filename", "")), repo_root)
        location = finding.get("location") or {}
        row = int(location.get("row", 0) or 0)
        if code == "T201":
            issues.append(
                {
                    "check": "architecture/print-in-production",
                    "severity": "warning",
                    "category": "Architecture",
                    "line": row,
                    "path": filename,
                    "message": "print() in production code - use logger instead",
                    "help": "Replace with logger.info/debug/warning as appropriate.",
                }
            )
        elif code == "F403":
            issues.append(
                {
                    "check": "architecture/star-import",
                    "severity": "warning",
                    "category": "Architecture",
                    "line": row,
                    "path": filename,
                    "message": str(
                        finding.get(
                            "message",
                            "from module import * - pollutes namespace and breaks static analysis",
                        )
                    ),
                    "help": "Import specific names instead of star imports.",
                }
            )
    return issues


def _merge_issues(
    native_issues: list[dict[str, Any]],
    extra_issues: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    merged = [dict(issue) for issue in native_issues]
    seen = {
        (issue["check"], str(issue.get("path", "")), int(issue.get("line", 0) or 0))
        for issue in merged
    }
    for issue in extra_issues:
        fingerprint = (issue["check"], str(issue.get("path", "")), int(issue.get("line", 0) or 0))
        if fingerprint in seen:
            continue
        seen.add(fingerprint)
        merged.append(dict(issue))
    return merged


def _build_requested_payload(args: argparse.Namespace) -> dict[str, Any]:
    return {
        "repo_root": args.repo_root,
        "code_dir": args.code_dir,
        "import_root": args.import_root,
        "app_module": args.app_module,
        "only_rules": args.only_rules,
        "ignore_rules": args.ignore_rules,
        "profile": args.profile,
        "fail_on": args.fail_on,
        "fail_on_tools": args.fail_on_tools,
        "include_tests": args.include_tests,
        "with_bandit": args.with_bandit,
        "with_tests": args.with_tests,
        "skip_ruff": args.skip_ruff,
        "skip_ty": args.skip_ty,
        "skip_structure": args.skip_structure,
        "skip_openapi": args.skip_openapi,
        "static_only": args.static_only,
        "skip_app_bootstrap": args.skip_app_bootstrap,
    }


def _resolved_tool_target(
    *,
    repo_root: Path,
    explicit_code_dir: str | None,
    doctor_report: dict[str, Any] | None,
) -> str:
    candidate = explicit_code_dir
    if candidate is None and doctor_report:
        project_context = doctor_report.get("project_context")
        if isinstance(project_context, dict):
            layout = project_context.get("layout")
            if isinstance(layout, dict):
                value = layout.get("code_dir")
                if isinstance(value, str) and value:
                    candidate = value
    if not candidate:
        return "."
    target = Path(candidate)
    if not target.is_absolute():
        target = (repo_root / target).resolve()
    try:
        relative = target.relative_to(repo_root)
    except ValueError:
        return target.as_posix()
    return "." if str(relative) == "." else relative.as_posix()


def _empty_doctor_report_from_scan_plan(scan_plan: dict[str, Any]) -> dict[str, Any]:
    return {
        "issues": [],
        "routes": [],
        "suppressions": [],
        "route_count": 0,
        "analyzed_file_count": 0,
        "openapi_path_count": None,
        "categories": {},
        "score": 100,
        "label": "A",
        "checks_not_evaluated": [],
        "engine_reason": "no rules selected",
        "project_context": scan_plan.get("project_context"),
    }


def _tool_requested(args: argparse.Namespace, tool_name: str) -> bool:
    return {
        "ruff": not args.skip_ruff,
        "ty": not args.skip_ty,
        "bandit": args.with_bandit,
        "pytest": args.with_tests,
    }[tool_name]


def _result_status(result: dict[str, Any] | None) -> tuple[str, str | None]:
    if not result:
        return "skipped", None
    stderr = str(result.get("stderr", ""))
    if result.get("status") == "command-not-found" or (
        "Failed to spawn:" in stderr and "No such file or directory" in stderr
    ):
        return "not_found", result.get("failure_reason") or "command-not-found"
    if result.get("passed"):
        return "passed", None
    return "failed", result.get("failure_reason") or "failed"


def _build_toolchain_status(
    args: argparse.Namespace,
    tool_results: dict[str, dict[str, Any]],
) -> dict[str, dict[str, Any]]:
    toolchain: dict[str, dict[str, Any]] = {}
    for tool_name in ("ruff", "ty", "bandit", "pytest"):
        requested = _tool_requested(args, tool_name)
        result = tool_results.get(tool_name)
        status, reason = _result_status(result if requested else None)
        entry: dict[str, Any] = {
            "requested": requested,
            "status": status if requested else "skipped",
            "reason": reason if requested else None,
        }
        if result:
            entry["command"] = result.get("command")
            entry["returncode"] = result.get("returncode")
        toolchain[tool_name] = entry
    return toolchain


def _build_score_components(
    doctor_report: dict[str, Any] | None,
    tool_results: dict[str, dict[str, Any]],
) -> dict[str, Any]:
    issues = doctor_report["issues"] if doctor_report else []
    error_rules = Counter(issue["check"] for issue in issues if issue["severity"] == "error")
    warning_rules = Counter(issue["check"] for issue in issues if issue["severity"] != "error")
    bandit_highs = _count_bandit_highs(tool_results["bandit"]["stdout"]) if "bandit" in tool_results else 0
    doctor_score = doctor_report["score"] if doctor_report else 100
    composite_score = max(0, doctor_score - min(15, bandit_highs * 5)) if tool_results else None
    return {
        "doctor_score": doctor_score,
        "composite_score": composite_score,
        "doctor_deductions": {
            "unique_error_rules": len(error_rules),
            "unique_warning_rules": len(warning_rules),
            "error_points": len(error_rules) * 2,
            "warning_points": len(warning_rules),
        },
        "toolchain_impact": {
            "bandit_high_findings": bandit_highs,
            "bandit_penalty_points": min(15, bandit_highs * 5),
        },
    }


def _issue_scope_bucket(issue_path: str, project_context: dict[str, Any] | None) -> str:
    if not issue_path:
        return "production"
    normalized = Path(issue_path).as_posix()
    parts = set(Path(normalized).parts)
    if "tests" in parts or "test" in parts:
        return "tests"
    scope = project_context.get("scope", {}) if isinstance(project_context, dict) else {}
    tool_scope = scope.get("tool_scope", {}) if isinstance(scope, dict) else {}
    excluded = set(tool_scope.get("exclude_dirs", []) if isinstance(tool_scope, dict) else [])
    if parts & excluded:
        return "excluded-but-tool-scanned"
    return "production"


def _build_diagnostics_summary(doctor_report: dict[str, Any] | None) -> dict[str, Any]:
    issues = doctor_report["issues"] if doctor_report else []
    error_rules = Counter(issue["check"] for issue in issues if issue["severity"] == "error")
    warning_rules = Counter(issue["check"] for issue in issues if issue["severity"] != "error")
    top_rules = Counter(issue["check"] for issue in issues).most_common(10)
    return {
        "total_findings": len(issues),
        "unique_error_rules": len(error_rules),
        "unique_warning_rules": len(warning_rules),
        "top_rule_families": [
            {"rule": rule, "count": count}
            for rule, count in top_rules
        ],
    }


def _resolve_tool_executable(name: str, repo_root: Path) -> str | None:
    local_candidates = [
        repo_root / ".venv" / "bin" / name,
        repo_root / ".venv" / "Scripts" / name,
        repo_root / ".venv" / "Scripts" / f"{name}.exe",
    ]
    for candidate in local_candidates:
        if candidate.exists() and os.access(candidate, os.X_OK):
            return candidate.as_posix()
    return shutil.which(name)


def _build_tool_jobs(
    *,
    args: argparse.Namespace,
    repo_root: Path,
    target: str | list[str],
    tool_exclude_dirs: list[str] | None = None,
) -> dict[str, tuple[str, list[str]]]:
    targets = [target] if isinstance(target, str) else list(target)
    tool_exclude_dirs = tool_exclude_dirs or []
    tool_jobs: dict[str, tuple[str, list[str]]] = {}
    if not args.skip_ruff:
        ruff_bin = _resolve_tool_executable("ruff", repo_root)
        ruff_cmd = [ruff_bin, "check", *targets, "--output-format", "json"] if ruff_bin else [
            "uvx",
            "ruff",
            "check",
            *targets,
            "--output-format",
            "json",
        ]
        if tool_exclude_dirs:
            ruff_cmd.extend(["--exclude", ",".join(tool_exclude_dirs)])
        tool_jobs["ruff"] = ("ruff", ruff_cmd)
    if not args.skip_ty:
        ty_bin = _resolve_tool_executable("ty", repo_root)
        ty_cmd = [ty_bin, "check", *targets, "--output-format", "concise"] if ty_bin else [
            "uvx",
            "ty",
            "check",
            *targets,
            "--output-format",
            "concise",
        ]
        tool_jobs["ty"] = ("ty", ty_cmd)
    if args.with_bandit:
        bandit_cmd = ["uv", "run", "bandit", "-q", "-f", "json", "-r", *targets]
        if tool_exclude_dirs:
            bandit_cmd.extend(["-x", ",".join(tool_exclude_dirs)])
        if (repo_root / "pyproject.toml").exists():
            bandit_cmd.extend(["-c", "pyproject.toml"])
        tool_jobs["bandit"] = ("bandit", bandit_cmd)
    if args.with_tests:
        tool_jobs["pytest"] = ("pytest", ["uv", "run", "pytest", *shlex.split(args.pytest_args)])
    return tool_jobs


def _build_json_payload(
    *,
    args: argparse.Namespace,
    command_results: list[dict[str, Any]],
    doctor_report: dict[str, Any] | None,
    doctor_score: int,
    composite_score: int | None,
    toolchain: dict[str, dict[str, Any]],
    score_components: dict[str, Any],
) -> dict[str, Any]:
    project_context = (doctor_report or {}).get("project_context") if doctor_report else None
    layout = project_context.get("layout", {}) if isinstance(project_context, dict) else {}
    effective_config = (
        project_context.get("effective_config", {})
        if isinstance(project_context, dict)
        else {}
    )
    scope = project_context.get("scope", {}) if isinstance(project_context, dict) else {}
    return {
        "schema_version": SCHEMA_VERSION,
        "score": doctor_score,
        "doctor_score": doctor_score,
        "composite_score": composite_score,
        "label": _label_for_score(doctor_score),
        "requested": _build_requested_payload(args),
        "project": {
            "repo_root": layout.get("repo_root"),
            "import_root": layout.get("import_root"),
            "code_dir": layout.get("code_dir"),
            "app_module": layout.get("app_module"),
            "discovery_source": layout.get("discovery_source"),
        },
        "scope": {
            "doctor_scope": scope.get("doctor_scope", {}),
            "tool_scope": scope.get("tool_scope", {}),
            "test_scope": {
                "enabled": args.with_tests,
                "pytest_args": args.pytest_args if args.with_tests else None,
            },
        },
        "effective_config": effective_config,
        "toolchain": toolchain,
        "score_components": score_components,
        "diagnostics_summary": _build_diagnostics_summary(doctor_report),
        "commands": command_results,
        "doctor": doctor_report,
    }


def _print_human_report(
    *,
    doctor_report: dict[str, Any] | None,
    toolchain: dict[str, dict[str, Any]],
    doctor_score: int,
    composite_score: int | None,
    verbose: bool,
) -> None:
    print(f"fastapi-doctor v{get_cli_version()}")
    print()
    print(f"Doctor score: {doctor_score}/100 {_label_for_score(doctor_score)}")
    if composite_score is not None:
        print(f"Composite score: {composite_score}/100 {_label_for_score(composite_score)}")
    if doctor_report:
        error_count = sum(1 for issue in doctor_report["issues"] if issue["severity"] == "error")
        warning_count = sum(
            1 for issue in doctor_report["issues"] if issue["severity"] != "error"
        )
        print(
            f"Findings: {error_count} errors, {warning_count} warnings, "
            f"{doctor_report['route_count']} routes, {doctor_report.get('analyzed_file_count', 0)} files"
        )
        project_context = doctor_report.get("project_context")
        scope = project_context.get("scope", {}) if isinstance(project_context, dict) else {}
        tool_scope = scope.get("tool_scope", {}) if isinstance(scope, dict) else {}
        if scope:
            print("Scan scope:")
            print(f"  doctor root: {scope.get('doctor_scope', {}).get('root', '.')}")
            print(f"  tool targets: {', '.join(tool_scope.get('targets', [])) or '.'}")
            if tool_scope.get("exclude_dirs"):
                print(f"  tool excludes: {', '.join(tool_scope['exclude_dirs'])}")
        if doctor_report.get("categories"):
            print("Categories:")
            for category, count in sorted(doctor_report["categories"].items()):
                print(f"  {category}: {count}")
        if doctor_report["issues"]:
            print("Issues:")
            seen_rules: set[str] = set()
            for issue in doctor_report["issues"]:
                if not verbose and issue["check"] in seen_rules:
                    continue
                seen_rules.add(issue["check"])
                location = issue["path"]
                if issue["line"]:
                    location = f"{location}:{issue['line']}"
                if verbose:
                    location = f"{location} [{_issue_scope_bucket(issue['path'], project_context)}]"
                print(f"  [{issue['check']}] {issue['message']}")
                print(f"    {location}")
                if verbose and issue.get("help"):
                    print(f"    {issue['help']}")
        else:
            print("No structural issues found.")
    if toolchain:
        print("Toolchain:")
        for tool_name, entry in toolchain.items():
            suffix = f" ({entry['reason']})" if entry.get("reason") else ""
            print(f"  {tool_name}: {entry['status']}{suffix}")
    elif not doctor_report:
        print("No checks were run.")


def _label_for_score(score: int) -> str:
    if score >= 80:
        return "Great"
    if score >= 60:
        return "Needs work"
    return "Critical"


def _should_fail_for_tools(
    args: argparse.Namespace,
    toolchain: dict[str, dict[str, Any]],
) -> bool:
    if args.fail_on_tools == "none":
        return False
    relevant = toolchain.values()
    return any(entry["status"] in {"failed", "not_found"} for entry in relevant)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Rust-native backend doctor for FastAPI and Python codebases."
    )
    parser.add_argument("-v", "--version", action="version", version=f"%(prog)s {get_cli_version()}")
    parser.add_argument("--list-rules", action="store_true", help="List all available rules and exit.")
    parser.add_argument("--init", action="store_true", help="Create a .fastapi-doctor.yml config file and exit.")
    parser.add_argument("--json", action="store_true", help="Emit machine-readable JSON output.")
    parser.add_argument(
        "--output-format",
        choices=["text", "json", "sarif", "github"],
        default=None,
        help="Output format: text (default), json, sarif, or github (annotations).",
    )
    parser.add_argument("--score", action="store_true", help="Output only the final score.")
    parser.add_argument("--explain-score", action="store_true", help="Show score breakdown.")
    parser.add_argument("--verbose", action="store_true", help="Show repeated findings.")
    parser.add_argument(
        "--fail-on",
        choices=["error", "warning", "none"],
        default="none",
        help="Exit non-zero on doctor diagnostics of the selected severity.",
    )
    parser.add_argument(
        "--fail-on-tools",
        choices=["none", "configured", "all"],
        default="none",
        help="Exit non-zero when external tools fail or are unavailable.",
    )
    parser.add_argument(
        "--profile",
        type=_normalize_profile,
        default="balanced",
        help="Audit profile: security focuses on boundary checks, balanced adds high-confidence correctness/API checks, strict adds broad architecture pressure.",
    )
    parser.add_argument("--ignore-rules", help="Comma-separated list of rule IDs or prefixes.")
    parser.add_argument("--only-rules", help="Comma-separated list of rule IDs or prefixes.")
    parser.add_argument(
        "--repo-root",
        help="Project root to scan. Defaults to $DOCTOR_REPO_ROOT or the current directory.",
    )
    parser.add_argument("--code-dir", help="Source directory to scan.")
    parser.add_argument("--import-root", help="Import root to add to the project context.")
    parser.add_argument("--app-module", help="FastAPI entrypoint module:attr or module:function.")
    parser.add_argument(
        "--include-tests",
        action="store_true",
        help="Include tests in native structural analysis for this run.",
    )
    parser.add_argument("--skip-ruff", action="store_true", help="Skip Ruff checks.")
    parser.add_argument("--skip-ty", action="store_true", help="Skip ty checks.")
    parser.add_argument("--skip-pyright", dest="skip_ty", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--skip-structure", action="store_true", help="Skip structural analysis.")
    parser.add_argument("--skip-openapi", action="store_true", help="Skip OpenAPI analysis.")
    parser.add_argument(
        "--static-only",
        action="store_true",
        help="Retained for compatibility; analysis is Rust-native and static.",
    )
    parser.add_argument(
        "--skip-app-bootstrap",
        action="store_true",
        help="Retained for compatibility; analysis is Rust-native and static.",
    )
    parser.add_argument("--with-bandit", action="store_true", help="Run Bandit alongside analysis.")
    parser.add_argument("--with-tests", action="store_true", help="Run pytest alongside analysis.")
    parser.add_argument(
        "--pytest-args",
        default="tests/ -q",
        help="Arguments passed to pytest when --with-tests is enabled.",
    )
    return parser.parse_args()


def _print_rule_table() -> int:
    try:
        rules = get_rule_metadata()
    except NativeEngineUnavailable as exc:
        print(str(exc), file=sys.stderr)
        return 1
    categories: dict[str, list[tuple[str, str]]] = {}
    for rule_id, severity, category in rules:
        categories.setdefault(category, []).append((rule_id, severity))
    for category in sorted(categories):
        print(f"\n{category}:")
        for rule_id, severity in sorted(categories[category]):
            print(f"  {rule_id:<50} {severity}")
    print(f"\n{len(rules)} rules total")
    return 0


_DEFAULT_CONFIG = """\
architecture:
  enabled: true
  giant_function: 400
  large_function: 200
  god_module: 1500
  deep_nesting: 5
  import_bloat: 30
  fat_route_handler: 100

pydantic:
  should_be_model: boundary

api:
  create_post_prefixes: []
  tag_required_prefixes:
    - /api/

security:
  forbidden_write_params: []
  auth_required_prefixes: []
  auth_dependency_names: []
  auth_exempt_prefixes:
    - /api/auth
    - /health
    - /ready
    - /live
    - /docs
    - /redoc
    - /openapi.json
    - /webhook
    - /oauth

scan:
  exclude_dirs:
    - .venv
    - venv
    - site-packages
    - __pycache__
    - node_modules
    - dist
    - build
    - .pytest_cache
    - .ruff_cache
    - .mypy_cache
    - tests
    - test
    - vendor
    - vendored
    - third_party
    - generated
  include_tests: false
  tool_include_dirs: []
  tool_exclude_dirs:
    - .venv
    - venv
    - site-packages
    - __pycache__
    - node_modules
    - dist
    - build
    - .pytest_cache
    - .ruff_cache
    - .mypy_cache
    - tests
    - test
    - vendor
    - vendored
    - third_party
    - generated
  exclude_rules: []
"""


def main() -> int:
    args = parse_args()

    if args.list_rules:
        return _print_rule_table()

    if args.init:
        config_path = Path(".fastapi-doctor.yml")
        if config_path.exists():
            print(f"{config_path} already exists", file=sys.stderr)
            return 1
        config_path.write_text(_DEFAULT_CONFIG)
        print(f"Created {config_path}")
        return 0

    if args.repo_root:
        os.environ["DOCTOR_REPO_ROOT"] = args.repo_root
    if args.code_dir:
        os.environ["DOCTOR_CODE_DIR"] = args.code_dir
    if args.import_root:
        os.environ["DOCTOR_IMPORT_ROOT"] = args.import_root
    if args.app_module:
        os.environ["DOCTOR_APP_MODULE"] = args.app_module
    if args.include_tests:
        os.environ["DOCTOR_INCLUDE_TESTS"] = "1"
    else:
        os.environ.pop("DOCTOR_INCLUDE_TESTS", None)

    repo_root = Path(args.repo_root or os.environ.get("DOCTOR_REPO_ROOT") or os.getcwd()).resolve()
    only_rules = set(_split_csv(args.only_rules) or [])
    ignore_rules = set(_split_csv(args.ignore_rules) or [])

    try:
        scan_session = create_scan_session(static_only=True)
        scan_plan = scan_session.get_scan_plan(
            profile=args.profile,
            only_rules=sorted(only_rules) if only_rules else None,
            ignore_rules=sorted(ignore_rules) if ignore_rules else None,
            skip_structure=args.skip_structure,
            skip_openapi=args.skip_openapi,
        )
    except NativeEngineUnavailable as exc:
        print(str(exc), file=sys.stderr)
        return 1

    native_result: dict[str, Any] | None = None
    tool_targets = scan_plan.get("tool_targets") or [str(scan_plan.get("tool_target", "."))]
    project_context = scan_plan.get("project_context") if isinstance(scan_plan, dict) else {}
    scope = project_context.get("scope", {}) if isinstance(project_context, dict) else {}
    tool_scope = scope.get("tool_scope", {}) if isinstance(scope, dict) else {}
    tool_jobs = _build_tool_jobs(
        args=args,
        repo_root=repo_root,
        target=tool_targets,
        tool_exclude_dirs=list(tool_scope.get("exclude_dirs", [])),
    )

    command_results: list[dict[str, Any]] = []
    tool_results: dict[str, dict[str, Any]] = {}
    native_requested = bool(scan_plan.get("native_requested", not (args.skip_structure and args.skip_openapi)))
    if tool_jobs:
        with ThreadPoolExecutor(max_workers=len(tool_jobs)) as pool:
            futures = {
                key: pool.submit(_run_command, name, command, repo_root)
                for key, (name, command) in tool_jobs.items()
            }
            if native_requested:
                try:
                    native_result = scan_session.analyze_selected_v2(
                        profile=args.profile,
                        only_rules=sorted(only_rules) if only_rules else None,
                        ignore_rules=sorted(ignore_rules) if ignore_rules else None,
                        skip_structure=args.skip_structure,
                        skip_openapi=args.skip_openapi,
                        include_routes=False,
                    )
                except NativeEngineUnavailable as exc:
                    print(str(exc), file=sys.stderr)
                    return 1
            elif not (args.skip_structure and args.skip_openapi):
                native_result = _empty_doctor_report_from_scan_plan(scan_plan)
            for key, future in futures.items():
                tool_results[key] = future.result()
        for key in ("ruff", "ty", "bandit", "pytest"):
            if key in tool_results:
                command_results.append(tool_results[key])
    elif native_requested:
        try:
            native_result = scan_session.analyze_selected_v2(
                profile=args.profile,
                only_rules=sorted(only_rules) if only_rules else None,
                ignore_rules=sorted(ignore_rules) if ignore_rules else None,
                skip_structure=args.skip_structure,
                skip_openapi=args.skip_openapi,
                include_routes=False,
            )
        except NativeEngineUnavailable as exc:
            print(str(exc), file=sys.stderr)
            return 1
    elif not (args.skip_structure and args.skip_openapi):
        native_result = _empty_doctor_report_from_scan_plan(scan_plan)

    ruff_issues: list[dict[str, Any]] = []
    if "ruff" in tool_results:
        ruff_issues = _map_ruff_findings_to_doctor(tool_results["ruff"]["stdout"], repo_root=repo_root)
    if native_result is not None:
        native_result["issues"] = _merge_issues(native_result["issues"], ruff_issues)

    toolchain = _build_toolchain_status(args, tool_results)
    score_components = _build_score_components(native_result, tool_results)
    doctor_score = int(score_components["doctor_score"])
    composite_score = score_components["composite_score"]

    if args.score:
        print(doctor_score)
        return 0

    if args.explain_score:
        all_issues = native_result["issues"] if native_result else []
        error_rules = Counter(issue["check"] for issue in all_issues if issue["severity"] == "error")
        warning_rules = Counter(issue["check"] for issue in all_issues if issue["severity"] != "error")
        print(f"Doctor score: {doctor_score}/100 ({_label_for_score(doctor_score)})")
        if error_rules:
            print(f"\nDoctor errors ({len(error_rules)} unique rules, -{len(error_rules) * 2} points):")
            for rule_id, count in sorted(error_rules.items()):
                print(f"  {rule_id} ({count} finding{'s' if count > 1 else ''})")
        if warning_rules:
            print(f"\nDoctor warnings ({len(warning_rules)} unique rules, -{len(warning_rules)} points):")
            for rule_id, count in sorted(warning_rules.items()):
                print(f"  {rule_id} ({count} finding{'s' if count > 1 else ''})")
        print("\nToolchain impact:")
        for tool_name, entry in toolchain.items():
            suffix = f" ({entry['reason']})" if entry.get("reason") else ""
            print(f"  {tool_name}: {entry['status']}{suffix}")
        bandit_highs = score_components["toolchain_impact"]["bandit_high_findings"]
        if bandit_highs:
            penalty = score_components["toolchain_impact"]["bandit_penalty_points"]
            print(f"  bandit high findings: {bandit_highs} (-{penalty} composite points)")
        print(
            f"\nComposite score: "
            f"{composite_score if composite_score is not None else doctor_score}/100 "
            f"({_label_for_score(composite_score if composite_score is not None else doctor_score)})"
        )
        return 0

    output_format = args.output_format or ("json" if args.json else "text")
    all_issues = native_result["issues"] if native_result else []

    if output_format == "sarif":
        from .sarif import to_sarif
        sarif_log = to_sarif(issues=all_issues, version=get_cli_version())
        print(json.dumps(sarif_log, indent=2))
    elif output_format == "github":
        from .sarif import to_github_annotations
        annotations = to_github_annotations(all_issues)
        if annotations:
            print(annotations)
    elif output_format == "json":
        payload = _build_json_payload(
            args=args,
            command_results=command_results,
            doctor_report=native_result,
            doctor_score=doctor_score,
            composite_score=composite_score,
            toolchain=toolchain,
            score_components=score_components,
        )
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        _print_human_report(
            doctor_report=native_result,
            toolchain=toolchain,
            doctor_score=doctor_score,
            composite_score=composite_score,
            verbose=args.verbose,
        )

    if args.fail_on != "none":
        has_error = bool(
            native_result and any(issue["severity"] == "error" for issue in native_result["issues"])
        )
        has_warning = bool(
            native_result and any(issue["severity"] != "error" for issue in native_result["issues"])
        )
        if args.fail_on == "error" and has_error:
            return 1
        if args.fail_on == "warning" and (has_error or has_warning):
            return 1

    structure_failed = bool(
        native_result and any(issue["severity"] == "error" for issue in native_result["issues"])
    )
    return 1 if structure_failed or _should_fail_for_tools(args, toolchain) else 0


if __name__ == "__main__":
    raise SystemExit(main())
