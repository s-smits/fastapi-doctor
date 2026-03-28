from __future__ import annotations

"""CLI entrypoint for the Rust-native fastapi-doctor package."""

import argparse
import json
import os
import shlex
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from importlib.metadata import PackageNotFoundError, version as metadata_version
from pathlib import Path
from typing import Any

from .native_core import NativeEngineUnavailable, analyze_selected_current_project_v2, get_rule_metadata

SCHEMA_VERSION = "1.2"
_SECURITY_SELECTORS = frozenset(
    {
        "security/*",
        "pydantic/sensitive-field-type",
        "pydantic/extra-allow-on-request",
        "config/direct-env-access",
    }
)
_MEDIUM_SELECTORS = _SECURITY_SELECTORS | frozenset(
    {
        "correctness/*",
        "resilience/*",
        "config/*",
        "pydantic/mutable-default",
        "pydantic/deprecated-validator",
        "architecture/async-without-await",
        "architecture/avoid-sys-exit",
        "architecture/engine-pool-pre-ping",
        "architecture/missing-startup-validation",
        "architecture/passthrough-function",
        "architecture/print-in-production",
        "api-surface/missing-pagination",
        "api-surface/missing-operation-id",
        "api-surface/duplicate-operation-id",
        "api-surface/missing-openapi-tags",
    }
)


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


def _runtime_rule_should_run(
    rule_id: str,
    *,
    profile: str | None,
    only_rules: set[str] | None,
    ignore_rules: set[str] | None,
) -> bool:
    if only_rules:
        return any(_matches_selector(rule_id, selector) for selector in only_rules)
    if profile == "security":
        if not any(_matches_selector(rule_id, selector) for selector in _SECURITY_SELECTORS):
            return False
    elif profile in {"balanced", "medium"}:
        if not any(_matches_selector(rule_id, selector) for selector in _MEDIUM_SELECTORS):
            return False
    if ignore_rules:
        return not any(_matches_selector(rule_id, selector) for selector in ignore_rules)
    return True


def _runtime_openapi_checks_not_evaluated(
    *,
    profile: str | None,
    only_rules: set[str] | None,
    ignore_rules: set[str] | None,
) -> list[str]:
    openapi_rule_ids = {
        "api-surface/missing-operation-id",
        "api-surface/duplicate-operation-id",
        "api-surface/missing-openapi-tags",
    }
    return sorted(
        rule_id
        for rule_id in openapi_rule_ids
        if _runtime_rule_should_run(
            rule_id,
            profile=profile,
            only_rules=only_rules,
            ignore_rules=ignore_rules,
        )
    )


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
    return stdout.count("Severity: High")


def _parse_ruff_json(stdout: str) -> list[dict[str, Any]]:
    try:
        payload = json.loads(stdout)
    except json.JSONDecodeError:
        return []
    return payload if isinstance(payload, list) else []


def _map_ruff_findings_to_doctor(stdout: str) -> list[dict[str, Any]]:
    findings = _parse_ruff_json(stdout)
    issues: list[dict[str, Any]] = []
    for finding in findings:
        if not isinstance(finding, dict):
            continue
        code = finding.get("code")
        filename = str(finding.get("filename", ""))
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


def _build_doctor_report(
    native_result: dict[str, Any] | None,
    ruff_issues: list[dict[str, Any]],
) -> dict[str, Any] | None:
    if native_result is None:
        return None
    doctor = dict(native_result)
    doctor["issues"] = native_result["issues"] + ruff_issues
    return doctor


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
        "with_bandit": args.with_bandit,
        "with_tests": args.with_tests,
        "skip_ruff": args.skip_ruff,
        "skip_ty": args.skip_ty,
        "skip_structure": args.skip_structure,
        "skip_openapi": args.skip_openapi,
        "static_only": args.static_only,
        "skip_app_bootstrap": args.skip_app_bootstrap,
    }


def _build_json_payload(
    *,
    args: argparse.Namespace,
    command_results: list[dict[str, Any]],
    doctor_report: dict[str, Any] | None,
    final_score: int,
    final_label: str,
) -> dict[str, Any]:
    project_context = (doctor_report or {}).get("project_context") if doctor_report else None
    layout = project_context.get("layout", {}) if isinstance(project_context, dict) else {}
    effective_config = (
        project_context.get("effective_config", {})
        if isinstance(project_context, dict)
        else {}
    )
    return {
        "schema_version": SCHEMA_VERSION,
        "score": final_score,
        "label": final_label,
        "requested": _build_requested_payload(args),
        "project": {
            "repo_root": layout.get("repo_root"),
            "import_root": layout.get("import_root"),
            "code_dir": layout.get("code_dir"),
            "app_module": layout.get("app_module"),
            "discovery_source": layout.get("discovery_source"),
        },
        "effective_config": effective_config,
        "commands": command_results,
        "doctor": doctor_report,
    }


def _print_human_report(
    *,
    doctor_report: dict[str, Any] | None,
    command_results: list[dict[str, Any]],
    final_score: int,
    final_label: str,
    verbose: bool,
) -> None:
    print(f"fastapi-doctor v{get_cli_version()}")
    print()
    print(f"Score: {final_score}/100 {final_label}")
    if doctor_report:
        error_count = sum(1 for issue in doctor_report["issues"] if issue["severity"] == "error")
        warning_count = sum(
            1 for issue in doctor_report["issues"] if issue["severity"] != "error"
        )
        print(
            f"Findings: {error_count} errors, {warning_count} warnings, "
            f"{doctor_report['route_count']} routes"
        )
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
                print(f"  [{issue['check']}] {issue['message']}")
                print(f"    {location}")
                if verbose and issue.get("help"):
                    print(f"    {issue['help']}")
        else:
            print("No structural issues found.")
    if command_results:
        print("External tools:")
        for result in command_results:
            status = "passed" if result["passed"] else result["status"]
            print(f"  {result['name']}: {status}")
    elif not doctor_report:
        print("No checks were run.")


def _compute_combined_score(
    base_score: int,
    ruff_passed: bool | None,
    ty_passed: bool | None,
    bandit_high_count: int | None,
) -> tuple[int, str]:
    score = float(base_score)
    if ruff_passed is False:
        score -= 5
    if ty_passed is False:
        score -= 5
    if bandit_high_count:
        score -= min(15, bandit_high_count * 5)
    final = max(0, min(100, int(score)))
    if final >= 80:
        return final, "Great"
    if final >= 60:
        return final, "Needs work"
    return final, "Critical"


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
        help="Exit non-zero on diagnostics of the selected severity.",
    )
    parser.add_argument(
        "--profile",
        type=_normalize_profile,
        default="balanced",
        help="Audit intensity profile: security, balanced, or strict. Legacy alias: medium.",
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

scan:
  exclude_dirs:
    - vendor
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

    repo_root = Path(args.repo_root or os.environ.get("DOCTOR_REPO_ROOT") or os.getcwd()).resolve()
    only_rules = set(_split_csv(args.only_rules) or [])
    ignore_rules = set(_split_csv(args.ignore_rules) or [])

    tool_jobs: dict[str, tuple[str, list[str]]] = {}
    if not args.skip_ruff:
        tool_jobs["ruff"] = ("ruff", ["uvx", "ruff", "check", ".", "--output-format", "json"])
    if not args.skip_ty:
        tool_jobs["ty"] = ("ty", ["uvx", "ty", "check", ".", "--output-format", "concise"])
    if args.with_bandit:
        bandit_cmd = ["uv", "run", "bandit", "-q", "-r", "."]
        if (repo_root / "pyproject.toml").exists():
            bandit_cmd.extend(["-c", "pyproject.toml"])
        tool_jobs["bandit"] = ("bandit", bandit_cmd)
    if args.with_tests:
        tool_jobs["pytest"] = ("pytest", ["uv", "run", "pytest", *shlex.split(args.pytest_args)])

    command_results: list[dict[str, Any]] = []
    tool_results: dict[str, dict[str, Any]] = {}
    if tool_jobs:
        with ThreadPoolExecutor(max_workers=len(tool_jobs)) as pool:
            futures = {
                key: pool.submit(_run_command, name, command, repo_root)
                for key, (name, command) in tool_jobs.items()
            }
            for key, future in futures.items():
                tool_results[key] = future.result()
        for key in ("ruff", "ty", "bandit", "pytest"):
            if key in tool_results:
                command_results.append(tool_results[key])

    ruff_issues: list[dict[str, Any]] = []
    if "ruff" in tool_results:
        ruff_issues = _map_ruff_findings_to_doctor(tool_results["ruff"]["stdout"])
        ignore_rules.update(issue["check"] for issue in ruff_issues)

    native_result: dict[str, Any] | None = None
    if not (args.skip_structure and args.skip_openapi):
        try:
            raw_native = analyze_selected_current_project_v2(
                profile=args.profile,
                only_rules=sorted(only_rules) if only_rules else None,
                ignore_rules=sorted(ignore_rules) if ignore_rules else None,
                skip_structure=args.skip_structure,
                skip_openapi=args.skip_openapi,
                static_only=True,
                include_routes=False,
            )
        except NativeEngineUnavailable as exc:
            print(str(exc), file=sys.stderr)
            return 1
        native_result = raw_native
        native_result["issues"] = native_result["issues"] + ruff_issues
        native_result["checks_not_evaluated"] = _runtime_openapi_checks_not_evaluated(
            profile=args.profile,
            only_rules=only_rules or None,
            ignore_rules=ignore_rules or None,
        )

    final_score, final_label = _compute_combined_score(
        native_result["score"] if native_result else 100,
        tool_results.get("ruff", {}).get("passed"),
        tool_results.get("ty", {}).get("passed"),
        _count_bandit_highs(tool_results["bandit"]["stdout"]) if "bandit" in tool_results else None,
    )

    if args.score:
        print(final_score)
        return 0

    if args.explain_score:
        all_issues = native_result["issues"] if native_result else []
        error_rules: dict[str, int] = {}
        warning_rules: dict[str, int] = {}
        for issue in all_issues:
            bucket = error_rules if issue["severity"] == "error" else warning_rules
            bucket[issue["check"]] = bucket.get(issue["check"], 0) + 1
        base_score = native_result["score"] if native_result else 100
        print(f"Base score: {base_score}/100")
        if error_rules:
            print(f"\nErrors ({len(error_rules)} unique rules, -{len(error_rules) * 2} points):")
            for rule_id, count in sorted(error_rules.items()):
                print(f"  {rule_id} ({count} finding{'s' if count > 1 else ''})")
        if warning_rules:
            print(f"\nWarnings ({len(warning_rules)} unique rules, -{len(warning_rules)} points):")
            for rule_id, count in sorted(warning_rules.items()):
                print(f"  {rule_id} ({count} finding{'s' if count > 1 else ''})")
        penalties = []
        ruff_passed = tool_results.get("ruff", {}).get("passed")
        ty_passed = tool_results.get("ty", {}).get("passed")
        if ruff_passed is False:
            penalties.append("ruff failed: -5")
        if ty_passed is False:
            penalties.append("ty failed: -5")
        bandit_highs = _count_bandit_highs(tool_results["bandit"]["stdout"]) if "bandit" in tool_results else 0
        if bandit_highs:
            penalties.append(f"bandit {bandit_highs} high findings: -{min(15, bandit_highs * 5)}")
        if penalties:
            print(f"\nTool penalties:")
            for p in penalties:
                print(f"  {p}")
        print(f"\nFinal score: {final_score}/100 ({final_label})")
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
            final_score=final_score,
            final_label=final_label,
        )
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        _print_human_report(
            doctor_report=native_result,
            command_results=command_results,
            final_score=final_score,
            final_label=final_label,
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

    has_command_failure = any(not result["passed"] for result in command_results)
    structure_failed = bool(
        native_result and any(issue["severity"] == "error" for issue in native_result["issues"])
    )
    return 1 if has_command_failure or structure_failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
