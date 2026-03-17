from __future__ import annotations

"""Resilience-focused static checks."""

import ast

from .. import project
from ..models import DoctorIssue

def check_bare_except_pass() -> list[DoctorIssue]:
    """except Exception: pass silently swallows errors — at minimum log them."""
    issues: list[DoctorIssue] = []
    for filepath in project.own_python_files():
        try:
            source = filepath.read_text()
            tree = ast.parse(source)
        except Exception:
            continue
        for node in ast.walk(tree):
            if not isinstance(node, ast.ExceptHandler):
                continue
            # Check if the handler body is just `pass` (or `pass` with a comment)
            if len(node.body) == 1 and isinstance(node.body[0], ast.Pass):
                # Check if there's a comment justification on the same line
                line_idx = node.lineno - 1
                lines = source.splitlines()
                has_comment = False
                # Check the except line and the pass line for comments
                for check_line in range(
                    max(0, line_idx), min(len(lines), (node.body[0].end_lineno or node.lineno) + 1)
                ):
                    if "#" in lines[check_line]:
                        has_comment = True
                        break
                if not has_comment:
                    issues.append(
                        DoctorIssue(
                            check="resilience/bare-except-pass",
                            severity="warning",
                            message="except: pass silently swallows errors without logging or comment",
                            path=str(filepath.relative_to(project.REPO_ROOT)),
                            category="Resilience",
                            help="Add logger.debug/warning or a # comment explaining why it's safe to ignore.",
                            line=node.lineno,
                        )
                    )
    return issues

def check_reraise_without_context() -> list[DoctorIssue]:
    """except handlers that re-raise without adding context are noise.

    AST-based detection: finds except handlers whose last statement is a bare
    ``raise`` (or ``raise e`` where ``e`` is the caught exception) AND the handler
    does no useful work (no logging, no cleanup, no variable assignment beyond
    the exception itself). If you catch just to re-raise, remove the try/except.
    If you need cleanup, add context with ``raise NewError(...) from exc``.
    """
    issues: list[DoctorIssue] = []
    for filepath in project.own_python_files():
        try:
            source = filepath.read_text()
            tree = ast.parse(source)
        except Exception:
            continue
        for node in ast.walk(tree):
            if not isinstance(node, ast.ExceptHandler):
                continue
            if not node.body:
                continue
            # Check if last statement is a bare raise
            last_stmt = node.body[-1]
            is_bare_raise = isinstance(last_stmt, ast.Raise) and last_stmt.exc is None
            is_identity_raise = False
            if isinstance(last_stmt, ast.Raise) and last_stmt.exc is not None:
                # `raise e` where e is the caught exception name
                if isinstance(last_stmt.exc, ast.Name) and last_stmt.exc.id == (node.name or ""):
                    is_identity_raise = True
            if not (is_bare_raise or is_identity_raise):
                continue
            # Check if there's any useful work before the raise
            preceding = node.body[:-1]
            has_useful_work = False
            for stmt in preceding:
                # Logging calls, assignments, function calls = useful work
                if isinstance(stmt, (ast.Assign, ast.AugAssign, ast.AnnAssign)):
                    has_useful_work = True
                    break
                if isinstance(stmt, ast.Expr) and isinstance(stmt.value, ast.Call):
                    has_useful_work = True
                    break
                if isinstance(stmt, (ast.If, ast.For, ast.While, ast.With, ast.Try)):
                    has_useful_work = True
                    break
            if has_useful_work:
                continue
            # Check for noqa comment
            lines = source.splitlines()
            if node.lineno <= len(lines) and "# noqa" in lines[node.lineno - 1]:
                continue
            issues.append(
                DoctorIssue(
                    check="resilience/reraise-without-context",
                    severity="warning",
                    message="except handler re-raises without adding context — remove the try/except or add info",
                    path=str(filepath.relative_to(project.REPO_ROOT)),
                    category="Resilience",
                    help="Either remove the try/except (it's noise) or use `raise NewError(...) from exc`.",
                    line=node.lineno,
                )
            )
    return issues


def check_exception_swallowed_silently() -> list[DoctorIssue]:
    """Detect except blocks that swallow exceptions without logging or re-raising.

    Patterns caught:
    - ``except Exception: pass``  (already caught by bare-except-pass for bare except)
    - ``except Exception: return <value>`` with no logging
    - ``except Exception as e:`` with body that never references ``e`` and has no logging

    These silently hide failures, making debugging extremely difficult.
    """
    _LOG_CALLS = {"logger", "logging", "log"}
    issues: list[DoctorIssue] = []
    for filepath in project.own_python_files():
        try:
            source = filepath.read_text()
            tree = ast.parse(source)
        except Exception:
            continue
        lines = source.splitlines()

        for node in ast.walk(tree):
            if not isinstance(node, ast.ExceptHandler):
                continue
            # Only flag 'except Exception' (not bare except, which is caught elsewhere)
            if not (isinstance(node.type, ast.Name) and node.type.id == "Exception"):
                continue
            # Skip if has noqa
            if node.lineno <= len(lines) and "# noqa" in lines[node.lineno - 1]:
                continue

            body = node.body
            if not body:
                continue

            # Check if exception variable is referenced in body
            exc_name = node.name  # e.g. 'e' in 'except Exception as e'
            body_source = ast.dump(ast.Module(body=body, type_ignores=[]))

            has_logging = False
            has_raise = False
            refs_exc = False

            for child in ast.walk(ast.Module(body=body, type_ignores=[])):
                # Check for logging calls
                if isinstance(child, ast.Call) and isinstance(child.func, ast.Attribute):
                    obj = child.func.value
                    if isinstance(obj, ast.Name) and obj.id in _LOG_CALLS:
                        has_logging = True
                    # Also check for logger.xxx(... exc_info=True)
                    if isinstance(obj, ast.Name):
                        for kw in child.keywords:
                            if kw.arg == "exc_info":
                                has_logging = True
                # Check for raise
                if isinstance(child, ast.Raise):
                    has_raise = True
                # Check if exception variable is referenced
                if exc_name and isinstance(child, ast.Name) and child.id == exc_name:
                    refs_exc = True

            # Swallowed = no logging AND no re-raise AND exception variable unused
            if not has_logging and not has_raise:
                # Only flag if: pass, return, or exc variable never used
                is_just_pass = len(body) == 1 and isinstance(body[0], ast.Pass)
                is_just_return = len(body) == 1 and isinstance(body[0], ast.Return)
                exc_unused = exc_name and not refs_exc

                if is_just_pass or is_just_return or exc_unused:
                    issues.append(DoctorIssue(
                        check="resilience/exception-swallowed",
                        severity="warning",
                        message="except Exception block swallows error without logging or re-raising",
                        path=str(filepath.relative_to(project.REPO_ROOT)),
                        category="Resilience",
                        help="Add logger.exception() or logger.warning(..., exc_info=True) to preserve debugging context.",
                        line=node.lineno,
                    ))
    return issues

def check_broad_except_no_context() -> list[DoctorIssue]:
    """Detect 'except Exception' that logs without exc_info context.

    Pattern: ``except Exception: logger.warning("something")`` without
    ``exc_info=True`` — the exception object is lost, making post-mortem
    debugging impossible. The logging call looks like it handles the error
    but actually discards the traceback.
    """
    issues: list[DoctorIssue] = []
    for filepath in project.own_python_files():
        try:
            source = filepath.read_text()
            tree = ast.parse(source)
        except Exception:
            continue
        lines = source.splitlines()

        for node in ast.walk(tree):
            if not isinstance(node, ast.ExceptHandler):
                continue
            if not (isinstance(node.type, ast.Name) and node.type.id == "Exception"):
                continue
            if node.lineno <= len(lines) and "# noqa" in lines[node.lineno - 1]:
                continue

            body = node.body
            has_raise = any(isinstance(c, ast.Raise) for c in ast.walk(ast.Module(body=body, type_ignores=[])))
            if has_raise:
                continue

            # Find logging calls without exc_info
            for child in ast.walk(ast.Module(body=body, type_ignores=[])):
                if not isinstance(child, ast.Call):
                    continue
                func = child.func
                if not isinstance(func, ast.Attribute):
                    continue
                obj = func.value
                if not (isinstance(obj, ast.Name) and obj.id in ("logger", "logging", "log")):
                    continue
                if func.attr not in ("warning", "warn", "info", "debug"):
                    continue
                # Check if exc_info=True is passed
                has_exc_info = any(
                    kw.arg == "exc_info" and isinstance(kw.value, ast.Constant) and kw.value.value is True
                    for kw in child.keywords
                )
                if not has_exc_info:
                    # Check if the exception variable is used as an argument
                    exc_name = node.name
                    refs_exc = False
                    if exc_name:
                        for arg in child.args:
                            if isinstance(arg, ast.Name) and arg.id == exc_name:
                                refs_exc = True
                            elif isinstance(arg, ast.JoinedStr):
                                for val in arg.values:
                                    if isinstance(val, ast.FormattedValue) and isinstance(val.value, ast.Name) and val.value.id == exc_name:
                                        refs_exc = True
                    if not refs_exc:
                        issues.append(DoctorIssue(
                            check="resilience/broad-except-no-context",
                            severity="warning",
                            message=f"except Exception logs via logger.{func.attr}() but discards traceback",
                            path=str(filepath.relative_to(project.REPO_ROOT)),
                            category="Resilience",
                            help="Add exc_info=True to the logging call or include the exception variable in the message.",
                            line=child.lineno,
                        ))
                    break  # Only flag once per except block
    return issues


__all__ = [
    "check_bare_except_pass",
    "check_broad_except_no_context",
    "check_exception_swallowed_silently",
    "check_reraise_without_context",
]
