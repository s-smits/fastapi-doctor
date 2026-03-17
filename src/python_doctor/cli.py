from __future__ import annotations

"""CLI entrypoint for fastapi-doctor."""

import argparse
import json
import os
from pathlib import Path

from .external_tools import CommandResult, count_bandit_highs, run_command
from .models import PERFECT_SCORE
from .project import get_project_layout
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

    command_results: list[CommandResult] = []

    # ── External tools ────────────────────────────────────────────────────
    ruff_passed = None
    pyright_passed = None
    bandit_high_count = None

    if not args.skip_ruff:
        result = run_command("ruff", ["uv", "run", "ruff", "check", "."], cwd=repo_root)
        command_results.append(result)
        ruff_passed = result.passed

    if not args.skip_pyright:
        result = run_command("pyright", ["uv", "run", "pyright"], cwd=repo_root)
        command_results.append(result)
        pyright_passed = result.passed

    if args.with_bandit:
        bandit_cmd = ["uv", "run", "bandit", "-q", "-r", "."]
        if (repo_root / "pyproject.toml").exists():
            bandit_cmd.extend(["-c", "pyproject.toml"])
        result = run_command("bandit", bandit_cmd, cwd=repo_root)
        command_results.append(result)
        bandit_high_count = count_bandit_highs(result.stdout)

    if args.with_tests:
        command_results.append(
            run_command("pytest", ["uv", "run", "pytest", *args.pytest_args.split()], cwd=repo_root)
        )

    # ── Doctor checks ─────────────────────────────────────────────────────
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

        doctor_report = run_python_doctor_checks(
            only_rules=only_rules if only_rules else None,
            ignore_rules=ignore_rules if ignore_rules else None,
        )

    # ── Combined score ────────────────────────────────────────────────────
    base_score = doctor_report.score if doctor_report else PERFECT_SCORE
    final_score, final_label = compute_combined_score(
        base_score, ruff_passed, pyright_passed, bandit_high_count
    )

    # ── Output ────────────────────────────────────────────────────────────
    if args.score:
        print(final_score)
        return 0

    if args.json:
        project_layout = get_project_layout()
        payload = {
            "score": final_score,
            "label": final_label,
            "project": {
                "repo_root": str(project_layout.repo_root),
                "import_root": str(project_layout.import_root),
                "code_dir": str(project_layout.code_dir),
                "app_module": project_layout.app_module,
                "discovery_source": project_layout.discovery_source,
            },
            "commands": [result.to_dict() for result in command_results],
            "doctor": doctor_report.to_dict() if doctor_report else None,
        }
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        print_report_human(doctor_report, command_results, final_score, final_label, verbose=args.verbose)

    # ── Fail on level ─────────────────────────────────────────────────────
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
    parser.add_argument("--json", action="store_true", help="Emit machine-readable JSON output.")
    parser.add_argument("--score", action="store_true", help="Output only the score.")
    parser.add_argument("--verbose", action="store_true", help="Show file details per rule.")
    parser.add_argument(
        "--fail-on",
        choices=["error", "warning", "none"],
        default="none",
        help="Exit with error code on diagnostics: error, warning, none.",
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

__all__ = ["main", "parse_args"]
