from __future__ import annotations

"""Human-readable report rendering."""

from typing import Any

from .external_tools import CommandResult


def _score_color(score: int) -> str:
    if score >= 80:
        return "\033[32m"  # Green
    if score >= 60:
        return "\033[33m"  # Yellow
    return "\033[31m"  # Red


def _reset() -> str:
    return "\033[0m"


def print_report_human(
    doctor_report: Any,
    command_results: list[CommandResult],
    final_score: int,
    final_label: str,
    verbose: bool = False,
) -> None:
    """Pretty-print the doctor report for humans."""
    color = _score_color(final_score)
    reset = _reset()

    # Score header
    print()
    print(f"  {color}{'━' * 50}{reset}")
    print(f"  {color}  Python Doctor Score: {final_score}/100 — {final_label}{reset}")
    print(f"  {color}{'━' * 50}{reset}")
    print()

    # External tools
    for result in command_results:
        status = f"\033[32mPASS{reset}" if result.passed else f"\033[31mFAIL{reset}"
        print(f"  [{status}] {result.name}")

    if doctor_report:
        print(f"\n  Routes: {doctor_report.route_count}  |  OpenAPI paths: {doctor_report.openapi_path_count}")
        print(f"  Errors: {doctor_report.error_count}  |  Warnings: {doctor_report.warning_count}")

        if doctor_report.categories:
            print(f"\n  {'Category':<20} {'Issues':>6}")
            print(f"  {'─' * 28}")
            for cat, count in doctor_report.categories.items():
                print(f"  {cat:<20} {count:>6}")

        # Show first few issues per category
        if doctor_report.issues:
            print(f"\n  {'─' * 60}")
            unique_shown_rules: set[str] = set()
            for issue in doctor_report.issues:
                is_new_rule = issue.check not in unique_shown_rules
                if not is_new_rule and not verbose:
                    continue
                unique_shown_rules.add(issue.check)

                severity_icon = "E" if issue.severity == "error" else "W"
                severity_color = "\033[31m" if issue.severity == "error" else "\033[33m"
                loc = f"{issue.path}"
                if issue.line:
                    loc += f":{issue.line}"
                
                if verbose:
                    print(f"  {severity_color}{severity_icon}{reset} [{issue.check}] {issue.message}")
                    print(f"    {loc}")
                else:
                    print(f"  {severity_color}{severity_icon}{reset} [{issue.check}] {issue.message}")
                
                if issue.help and (is_new_rule or verbose):
                    print(f"    → {issue.help}")
                
                if not verbose:
                    # Count occurrences
                    count = sum(1 for i in doctor_report.issues if i.check == issue.check)
                    if count > 1:
                        print(f"    ({count} occurrences)")
                print()

    print()

__all__ = ["print_report_human"]
