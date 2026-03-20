from __future__ import annotations

"""CLI entrypoint for fastapi-doctor."""

import argparse
import json
import os
from importlib.metadata import PackageNotFoundError, version
from pathlib import Path

from .console import logger
from .external_tools import CommandResult, count_bandit_highs, run_command
from .models import PERFECT_SCORE, SCHEMA_VERSION
from .project import get_effective_config, get_project_layout
from .reporting import print_report_human
from .runner import run_python_doctor_checks


def compute_combined_score(
    base_score: int,
    ruff_passed: bool | None,
    pyright_passed: bool | None,
    bandit_high_count: int | None,
) -> tuple[int, str]:
    """Adjust structural score based on external tool signals."""
    score = float(base_score)

    # Ruff failure (any error) is a major signal
    if ruff_passed is False:
        score -= 5

    # Pyright failure (type errors) is a major signal
    if pyright_passed is False:
        score -= 5

    # Bandit high-severity issues
    if bandit_high_count:
        score -= min(15, bandit_high_count * 5)

    final = max(0, min(100, int(score)))

    if final >= 80:
        label = "Great"
    elif final >= 60:
        label = "Needs work"
    else:
        label = "Critical"

    return final, label

def main() -> int:
    args = parse_args()
    configure_environment_from_args(args)
    repo_root = resolve_repo_root()
    cli_version = get_cli_version()

    command_results: list[CommandResult] = []

    if not args.json and not args.score:
        logger.log(f"fastapi-doctor v{cli_version}")
        logger.break_line()

    ruff_passed = None
    pyright_passed = None
    bandit_high_count = None

    # Build list of external tool jobs to run concurrently.
    from concurrent.futures import ThreadPoolExecutor

    tool_jobs: dict[str, tuple[str, list[str]]] = {}
    if not args.skip_ruff:
        tool_jobs["ruff"] = ("ruff", ["uv", "run", "ruff", "check", "."])
    if not args.skip_pyright:
        tool_jobs["pyright"] = ("pyright", ["uv", "run", "pyright"])
    if args.with_bandit:
        bandit_cmd = ["uv", "run", "bandit", "-q", "-r", "."]
        if (repo_root / "pyproject.toml").exists():
            bandit_cmd.extend(["-c", "pyproject.toml"])
        tool_jobs["bandit"] = ("bandit", bandit_cmd)
    if args.with_tests:
        tool_jobs["pytest"] = ("pytest", ["uv", "run", "pytest", *args.pytest_args.split()])

    if tool_jobs:
        quiet = args.json or args.score
        if not quiet:
            logger.dim(f"Running {', '.join(tool_jobs)}...")

        tool_results: dict[str, CommandResult] = {}
        with ThreadPoolExecutor(max_workers=len(tool_jobs)) as pool:
            futures = {
                key: pool.submit(run_command, name, cmd, cwd=repo_root)
                for key, (name, cmd) in tool_jobs.items()
            }
            for key, future in futures.items():
                tool_results[key] = future.result()

        # Collect results in display order.
        for key in ("ruff", "pyright", "bandit", "pytest"):
            if key in tool_results:
                command_results.append(tool_results[key])

        ruff_passed = tool_results["ruff"].passed if "ruff" in tool_results else None
        pyright_passed = tool_results["pyright"].passed if "pyright" in tool_results else None
        if "bandit" in tool_results:
            bandit_high_count = count_bandit_highs(tool_results["bandit"].stdout)

        if not quiet:
            logger.break_line()

    doctor_report = None
    if not args.skip_structure or not args.skip_openapi:
        only_rules = set(args.only_rules.split(",")) if args.only_rules else set()
        ignore_rules = set(args.ignore_rules.split(",")) if args.ignore_rules else set()

        if args.skip_structure:
            ignore_rules.add("architecture/")
            ignore_rules.add("correctness/")
            ignore_rules.add("pydantic/")
            ignore_rules.add("resilience/")
            ignore_rules.add("security/")
            ignore_rules.add("config/")
        if args.skip_openapi:
            ignore_rules.add("api-surface/")

        if not args.json and not args.score:
            logger.dim("Running FastAPI Doctor checks...")
            logger.break_line()
        doctor_report = run_python_doctor_checks(
            only_rules=only_rules if only_rules else None,
            ignore_rules=ignore_rules if ignore_rules else None,
            profile=args.profile,
        )

    base_score = doctor_report.score if doctor_report else PERFECT_SCORE
    final_score, final_label = compute_combined_score(
        base_score, ruff_passed, pyright_passed, bandit_high_count
    )

    if args.score:
        print(final_score)
        return 0

    if args.json:
        payload = build_json_payload(
            args=args,
            command_results=command_results,
            doctor_report=doctor_report,
            final_score=final_score,
            final_label=final_label,
        )
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        print_report_human(doctor_report, command_results, final_score, final_label, verbose=args.verbose)

    if args.fail_on != "none":
        has_error = bool(doctor_report and doctor_report.error_count > 0)
        has_warning = bool(doctor_report and doctor_report.warning_count > 0)
        if args.fail_on == "error" and has_error:
            return 1
        if args.fail_on == "warning" and (has_error or has_warning):
            return 1

    has_command_failure = any(not result.passed for result in command_results)
    structure_failed = bool(doctor_report and doctor_report.error_count)
    return 1 if has_command_failure or structure_failed else 0

def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Backend doctor for your FastAPI/Python stack. Scores 0-100."
    )
    parser.add_argument(
        "-v",
        "--version",
        action="version",
        version=f"%(prog)s {get_cli_version()}",
    )
    parser.add_argument("--json", action="store_true", help="Emit machine-readable JSON output.")
    parser.add_argument("--score", action="store_true", help="Output only the score.")
    parser.add_argument("--verbose", action="store_true", help="Show file details per rule.")
    parser.add_argument(
        "--fail-on",
        choices=["error", "warning", "none"],
        default="none",
        help="Exit with error code on diagnostics: error, warning, none.",
    )
    parser.add_argument(
        "--profile",
        choices=["security", "medium", "strict"],
        default="medium",
        help="Audit intensity profile: security (only security checks), medium (balanced), strict (all checks).",
    )
    parser.add_argument("--ignore-rules", help="Comma-separated list of rule IDs or prefixes to ignore.")
    parser.add_argument("--only-rules", help="Comma-separated list of rule IDs or prefixes to run.")
    parser.add_argument("--repo-root", help="Project root to scan. Defaults to $DOCTOR_REPO_ROOT or the current working directory.")
    parser.add_argument("--code-dir", help="Package/source directory to scan. Defaults to auto-discovery.")
    parser.add_argument("--import-root", help="Directory added to sys.path for importing the app module. Defaults to auto-discovery.")
    parser.add_argument(
        "--app-module",
        help="FastAPI entrypoint as module:attribute or module:function(). Defaults to auto-discovery.",
    )
    parser.add_argument("--skip-ruff", action="store_true")
    parser.add_argument("--skip-pyright", action="store_true")
    parser.add_argument("--skip-structure", action="store_true")
    parser.add_argument("--skip-openapi", action="store_true")
    parser.add_argument("--with-bandit", action="store_true", help="Include Bandit security scan.")
    parser.add_argument("--with-tests", action="store_true", help="Run targeted backend test suites.")
    parser.add_argument(
        "--pytest-args",
        default="tests/ -q",
        help="Arguments passed to pytest when --with-tests is enabled.",
    )
    return parser.parse_args()


def get_cli_version() -> str:
    try:
        return version("fastapi-doctor")
    except PackageNotFoundError:
        return "0.1.0"


def build_json_payload(
    *,
    args: argparse.Namespace,
    command_results: list[CommandResult],
    doctor_report: object | None,
    final_score: int,
    final_label: str,
) -> dict[str, object]:
    project_layout = get_project_layout()
    return {
        "schema_version": SCHEMA_VERSION,
        "score": final_score,
        "label": final_label,
        "requested": {
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
            "skip_pyright": args.skip_pyright,
            "skip_structure": args.skip_structure,
            "skip_openapi": args.skip_openapi,
        },
        "project": {
            "repo_root": str(project_layout.repo_root),
            "import_root": str(project_layout.import_root),
            "code_dir": str(project_layout.code_dir),
            "app_module": project_layout.app_module,
            "discovery_source": project_layout.discovery_source,
        },
        "effective_config": get_effective_config(),
        "commands": [result.to_dict() for result in command_results],
        "doctor": doctor_report.to_dict() if doctor_report else None,
    }


def configure_environment_from_args(args: argparse.Namespace) -> None:
    mappings = {
        "DOCTOR_REPO_ROOT": args.repo_root,
        "DOCTOR_CODE_DIR": args.code_dir,
        "DOCTOR_IMPORT_ROOT": args.import_root,
        "DOCTOR_APP_MODULE": args.app_module,
    }
    for env_name, value in mappings.items():
        if value:
            os.environ[env_name] = value


def resolve_repo_root() -> Path:
    raw_root = os.environ.get("DOCTOR_REPO_ROOT") or os.getcwd()
    return Path(raw_root).resolve()

__all__ = ["build_json_payload", "main", "parse_args"]
