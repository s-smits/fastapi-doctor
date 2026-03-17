from __future__ import annotations

"""Console rendering helpers modeled after react-doctor's CLI output."""

import os
import sys
from dataclasses import dataclass

from .models import PERFECT_SCORE, SCORE_GOOD_THRESHOLD, SCORE_OK_THRESHOLD

SUMMARY_BOX_HORIZONTAL_PADDING_CHARS = 1
SUMMARY_BOX_OUTER_INDENT_CHARS = 2
SCORE_BAR_WIDTH_CHARS = 50


def _supports_color() -> bool:
    if os.environ.get("NO_COLOR"):
        return False
    if os.environ.get("FORCE_COLOR"):
        return True
    return sys.stdout.isatty()


USE_COLOR = _supports_color()


def _paint(text: str, code: str) -> str:
    if not USE_COLOR:
        return text
    return f"\033[{code}m{text}\033[0m"


class highlighter:
    @staticmethod
    def error(text: str) -> str:
        return _paint(text, "31")

    @staticmethod
    def warn(text: str) -> str:
        return _paint(text, "33")

    @staticmethod
    def info(text: str) -> str:
        return _paint(text, "36")

    @staticmethod
    def success(text: str) -> str:
        return _paint(text, "32")

    @staticmethod
    def dim(text: str) -> str:
        return _paint(text, "2")


class logger:
    @staticmethod
    def error(*args: object) -> None:
        print(highlighter.error(" ".join(str(arg) for arg in args)))

    @staticmethod
    def warn(*args: object) -> None:
        print(highlighter.warn(" ".join(str(arg) for arg in args)))

    @staticmethod
    def info(*args: object) -> None:
        print(highlighter.info(" ".join(str(arg) for arg in args)))

    @staticmethod
    def success(*args: object) -> None:
        print(highlighter.success(" ".join(str(arg) for arg in args)))

    @staticmethod
    def dim(*args: object) -> None:
        print(highlighter.dim(" ".join(str(arg) for arg in args)))

    @staticmethod
    def log(*args: object) -> None:
        print(" ".join(str(arg) for arg in args))

    @staticmethod
    def break_line() -> None:
        print()


def colorize_by_score(text: str, score: int) -> str:
    if score >= SCORE_GOOD_THRESHOLD:
        return highlighter.success(text)
    if score >= SCORE_OK_THRESHOLD:
        return highlighter.warn(text)
    return highlighter.error(text)


def get_doctor_face(score: int) -> tuple[str, str]:
    if score >= SCORE_GOOD_THRESHOLD:
        return "◠ ◠", " ▽ "
    if score >= SCORE_OK_THRESHOLD:
        return "• •", " ─ "
    return "x x", " ▽ "


def build_score_bar(score: int) -> tuple[str, str]:
    filled_count = round((score / PERFECT_SCORE) * SCORE_BAR_WIDTH_CHARS)
    empty_count = SCORE_BAR_WIDTH_CHARS - filled_count
    filled = "█" * filled_count
    empty = "░" * empty_count
    return filled + empty, colorize_by_score(filled, score) + highlighter.dim(empty)


@dataclass(slots=True)
class FramedLine:
    plain_text: str
    rendered_text: str


def create_framed_line(plain_text: str, rendered_text: str | None = None) -> FramedLine:
    return FramedLine(plain_text=plain_text, rendered_text=rendered_text or plain_text)


def render_framed_box_string(framed_lines: list[FramedLine]) -> str:
    if not framed_lines:
        return ""

    outer_indent = " " * SUMMARY_BOX_OUTER_INDENT_CHARS
    horizontal_padding = " " * SUMMARY_BOX_HORIZONTAL_PADDING_CHARS
    max_line_length = max(len(line.plain_text) for line in framed_lines)
    border = "─" * (max_line_length + SUMMARY_BOX_HORIZONTAL_PADDING_CHARS * 2)

    lines = [f"{outer_indent}{highlighter.dim(f'┌{border}┐')}"]
    for framed_line in framed_lines:
        trailing_spaces = " " * (max_line_length - len(framed_line.plain_text))
        lines.append(
            f"{outer_indent}{highlighter.dim('│')}"
            f"{horizontal_padding}{framed_line.rendered_text}{trailing_spaces}{horizontal_padding}"
            f"{highlighter.dim('│')}"
        )
    lines.append(f"{outer_indent}{highlighter.dim(f'└{border}┘')}")
    return "\n".join(lines)


def print_framed_box(framed_lines: list[FramedLine]) -> None:
    rendered = render_framed_box_string(framed_lines)
    if rendered:
        logger.log(rendered)


__all__ = [
    "FramedLine",
    "build_score_bar",
    "colorize_by_score",
    "create_framed_line",
    "get_doctor_face",
    "highlighter",
    "logger",
    "print_framed_box",
    "render_framed_box_string",
]
