from __future__ import annotations

"""Human-readable report rendering."""

from typing import Any

from .console import (
    build_score_bar,
    colorize_by_score,
    create_framed_line,
    get_doctor_face,
    highlighter,
    logger,
    print_framed_box,
)
from .external_tools import CommandResult


def _colorize_command_status(result: CommandResult) -> str:
    if result.passed:
        return highlighter.success("✓")
    return highlighter.error("✗")


def _colorize_issue_icon(severity: str) -> str:
    if severity == "error":
        return highlighter.error("✗")
    return highlighter.warn("⚠")


def _format_location(path: str, line: int) -> str:
    if line:
        return f"{path}:{line}"
    return path


def _print_summary_box(
    doctor_report: Any,
    final_score: int,
    final_label: str,
    command_results: list[CommandResult],
) -> None:
    eyes, mouth = get_doctor_face(final_score)
    plain_score_bar, rendered_score_bar = build_score_bar(final_score)

    lines = [
        create_framed_line("┌─────┐", colorize_by_score("┌─────┐", final_score)),
        create_framed_line(f"│ {eyes} │", colorize_by_score(f"│ {eyes} │", final_score)),
        create_framed_line(f"│ {mouth} │", colorize_by_score(f"│ {mouth} │", final_score)),
        create_framed_line("└─────┘", colorize_by_score("└─────┘", final_score)),
        create_framed_line(
            "FastAPI Doctor",
            f"FastAPI Doctor {highlighter.dim('(Python backend health scan)')}",
        ),
        create_framed_line(""),
        create_framed_line(
            f"{final_score} / 100  {final_label}",
            f"{colorize_by_score(str(final_score), final_score)} / 100  {colorize_by_score(final_label, final_score)}",
        ),
        create_framed_line(""),
        create_framed_line(plain_score_bar, rendered_score_bar),
        create_framed_line(""),
    ]

    if doctor_report:
        error_count = doctor_report.error_count
        warning_count = doctor_report.warning_count
        routes_text = f"{doctor_report.route_count} routes"
        openapi_text = f"{doctor_report.openapi_path_count} OpenAPI paths"
        plain_parts: list[str] = []
        rendered_parts: list[str] = []
        if error_count:
            text = f"✗ {error_count} error{'s' if error_count != 1 else ''}"
            plain_parts.append(text)
            rendered_parts.append(highlighter.error(text))
        if warning_count:
            text = f"⚠ {warning_count} warning{'s' if warning_count != 1 else ''}"
            plain_parts.append(text)
            rendered_parts.append(highlighter.warn(text))
        plain_parts.extend([routes_text, openapi_text])
        rendered_parts.extend([highlighter.dim(routes_text), highlighter.dim(openapi_text)])
        lines.append(create_framed_line("  ".join(plain_parts), "  ".join(rendered_parts)))

    if command_results:
        failed = sum(1 for result in command_results if not result.passed)
        tools_label = f"{len(command_results)} tool{'s' if len(command_results) != 1 else ''}"
        status_label = "all passed" if failed == 0 else f"{failed} failed"
        lines.append(
            create_framed_line(
                f"{tools_label}  {status_label}",
                f"{highlighter.dim(tools_label)}  {highlighter.success(status_label) if failed == 0 else highlighter.error(status_label)}",
            )
        )

    print_framed_box(lines)


def print_report_human(
    doctor_report: Any,
    command_results: list[CommandResult],
    final_score: int,
    final_label: str,
    verbose: bool = False,
) -> None:
    """Pretty-print the doctor report for humans."""
    logger.break_line()
    _print_summary_box(doctor_report, final_score, final_label, command_results)
    logger.break_line()

    for result in command_results:
        icon = _colorize_command_status(result)
        command_str = " ".join(result.command)
        outcome = highlighter.success("passed") if result.passed else highlighter.error(
            result.failure_reason or f"failed ({result.returncode})"
        )
        logger.log(f"  {icon} {result.name}  {outcome}")
        logger.dim(f"    {command_str}")

    if doctor_report:
        logger.break_line()
        logger.log("  Findings")
        logger.dim("  Sorted by category impact and collapsed by rule unless --verbose is set.")
        logger.break_line()

        if doctor_report.categories:
            logger.log("  Categories")
            for cat, count in doctor_report.categories.items():
                logger.log(f"    {highlighter.info(cat)} {highlighter.dim(str(count))}")
            logger.break_line()

        if doctor_report.issues:
            unique_shown_rules: set[str] = set()
            for issue in doctor_report.issues:
                is_new_rule = issue.check not in unique_shown_rules
                if not is_new_rule and not verbose:
                    continue
                unique_shown_rules.add(issue.check)

                icon = _colorize_issue_icon(issue.severity)
                location = _format_location(issue.path, issue.line)
                count = sum(1 for item in doctor_report.issues if item.check == issue.check)
                count_label = f" ({count})" if count > 1 else ""

                logger.log(f"  {icon} [{issue.check}] {issue.message}{count_label}")
                logger.dim(f"    {location}")
                if verbose:
                    logger.dim(f"    category: {issue.category}")
                if issue.help:
                    logger.dim(f"    {issue.help}")
                logger.break_line()
        else:
            logger.success("  No structural issues found.")
            logger.break_line()

    if not doctor_report and not command_results:
        logger.dim("  No checks were run.")
        logger.break_line()

__all__ = ["print_report_human"]
