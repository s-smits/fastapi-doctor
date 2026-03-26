from __future__ import annotations

"""PyO3 native extension integration for static checks.

This keeps FastAPI app loading and report assembly in Python while the
static rule engine runs as a compiled Rust extension via PyO3.
"""

import os

from . import project
from .models import DoctorIssue

NATIVE_STATIC_RULES = frozenset(
    {
        "architecture/giant-function",
        "architecture/deep-nesting",
        "architecture/async-without-await",
        "architecture/import-bloat",
        "architecture/print-in-production",
        "architecture/star-import",
        "architecture/god-module",
        "architecture/passthrough-function",
        "architecture/avoid-sys-exit",
        "architecture/engine-pool-pre-ping",
        "architecture/missing-startup-validation",
        "architecture/fat-route-handler",
        "config/direct-env-access",
        "correctness/asyncio-run-in-async",
        "correctness/sync-io-in-async",
        "correctness/misused-async-constructs",
        "correctness/avoid-os-path",
        "correctness/deprecated-typing-imports",
        "correctness/mutable-default-arg",
        "correctness/naive-datetime",
        "correctness/return-in-finally",
        "correctness/threading-lock-in-async",
        "correctness/unreachable-code",
        "correctness/get-with-side-effect",
        "correctness/serverless-filesystem-write",
        "correctness/missing-http-timeout",
        "performance/heavy-imports",
        "performance/sequential-awaits",
        "performance/regex-in-loop",
        "performance/n-plus-one-hint",
        "pydantic/deprecated-validator",
        "pydantic/mutable-default",
        "pydantic/extra-allow-on-request",
        "pydantic/should-be-model",
        "pydantic/sensitive-field-type",
        "security/assert-in-production",
        "security/cors-wildcard",
        "security/exception-detail-leak",
        "security/subprocess-shell-true",
        "security/unsafe-yaml-load",
        "security/weak-hash-without-flag",
        "security/sql-fstring-interpolation",
        "security/hardcoded-secret",
        "security/pydantic-secretstr",
        "resilience/sqlalchemy-pool-pre-ping",
        "resilience/bare-except-pass",
        "resilience/reraise-without-context",
        "resilience/exception-swallowed",
        "resilience/broad-except-no-context",
    }
)

_LAST_NATIVE_REASON = "native not evaluated yet"


def _set_last_native_reason(reason: str) -> None:
    global _LAST_NATIVE_REASON
    _LAST_NATIVE_REASON = reason


def last_native_reason() -> str:
    return _LAST_NATIVE_REASON


def _native_enabled() -> bool:
    value = os.environ.get("FASTAPI_DOCTOR_DISABLE_NATIVE", "").strip().lower()
    return value not in {"1", "true", "yes", "on"}


def run_native_static_checks(requested_rules: set[str]) -> list[DoctorIssue] | None:
    """Run the native PyO3 extension for supported rules, or return None on fallback."""
    if not requested_rules:
        return []
    if not _native_enabled():
        _set_last_native_reason("native disabled by FASTAPI_DOCTOR_DISABLE_NATIVE")
        return None

    try:
        from . import _fastapi_doctor_native
    except ImportError as e:
        _set_last_native_reason(f"failed to import native PyO3 module: {e}")
        return None

    modules = project.parsed_python_modules()
    module_args = [(module.rel_path, module.source) for module in modules]
    active_rules = sorted(requested_rules)



    try:
        raw_issues = _fastapi_doctor_native.analyze_modules(
            project._IMPORT_BLOAT_THRESHOLD,
            project.GIANT_FUNCTION_THRESHOLD,
            project.LARGE_FUNCTION_THRESHOLD,
            project.DEEP_NESTING_THRESHOLD,
            project.GOD_MODULE_THRESHOLD,
            project._FAT_ROUTE_HANDLER_THRESHOLD,
            project.SHOULD_BE_MODEL_MODE,
            active_rules,
            module_args,
        )
    except Exception as e:
        _set_last_native_reason(f"native execution failed: {e}")
        return None

    issues: list[DoctorIssue] = []
    for check, severity, category, line, path, message, help_text in raw_issues:
        issues.append(
            DoctorIssue(
                check=check,
                severity=severity,
                message=message,
                path=path,
                category=category,
                help=help_text,
                line=line,
            )
        )

    _set_last_native_reason("using PyO3 native module")
    return issues


__all__ = [
    "NATIVE_STATIC_RULES",
    "last_native_reason",
    "run_native_static_checks",
]
