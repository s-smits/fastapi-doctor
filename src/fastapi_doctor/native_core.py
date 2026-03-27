from __future__ import annotations

"""Thin adapter over the PyO3 native static analysis engine."""

import os
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .models import DoctorIssue
    from .static_routes import RouteInfo

_LAST_NATIVE_REASON = "native not evaluated yet"


def _set_last_native_reason(reason: str) -> None:
    global _LAST_NATIVE_REASON
    _LAST_NATIVE_REASON = reason


def last_native_reason() -> str:
    return _LAST_NATIVE_REASON


def _native_enabled() -> bool:
    value = os.environ.get("FASTAPI_DOCTOR_DISABLE_NATIVE", "").strip().lower()
    return value not in {"1", "true", "yes", "on"}


def get_native_rule_ids() -> frozenset[str]:
    """Return the set of rule IDs owned by the Rust engine."""
    try:
        from . import _fastapi_doctor_native
        return frozenset(_fastapi_doctor_native.get_all_rule_ids())
    except Exception:
        return frozenset()


def run_native_project_v2(active_rules: set[str], *, include_routes: bool = True) -> dict | None:
    """Run the full static project analysis via the native Rust engine.

    Returns a result dict with keys: issues, routes, suppressions, route_count,
    categories, score, label, checks_not_evaluated. Returns None on fallback.
    """
    if not active_rules:
        return {
            "issues": [],
            "routes": [],
            "suppressions": [],
            "route_count": 0,
            "openapi_path_count": None,
            "categories": {},
            "score": 100,
            "label": "A",
            "checks_not_evaluated": [],
            "engine_reason": "no rules selected",
        }
    if not _native_enabled():
        _set_last_native_reason("native disabled by FASTAPI_DOCTOR_DISABLE_NATIVE")
        return None

    try:
        from . import _fastapi_doctor_native
        from .models import DoctorIssue
        from .static_routes import RouteInfo
    except ImportError as e:
        _set_last_native_reason(f"failed to import native PyO3 module: {e}")
        return None

    from . import project

    try:
        raw = _fastapi_doctor_native.analyze_project_v2(
            str(project.REPO_ROOT),
            str(project.OWN_CODE_DIR),
            sorted(project.SCAN_EXCLUDED_DIRS),
            project._IMPORT_BLOAT_THRESHOLD,
            project.GIANT_FUNCTION_THRESHOLD,
            project.LARGE_FUNCTION_THRESHOLD,
            project.DEEP_NESTING_THRESHOLD,
            project.GOD_MODULE_THRESHOLD,
            project._FAT_ROUTE_HANDLER_THRESHOLD,
            project.SHOULD_BE_MODEL_MODE,
            sorted(active_rules),
            include_routes,
        )
    except Exception as e:
        _set_last_native_reason(f"native execution failed: {e}")
        return None

    issues: list[DoctorIssue] = [
        DoctorIssue(
            check=check,
            severity=severity,
            message=message,
            path=path,
            category=category,
            help=help_text,
            line=line,
        )
        for check, severity, category, line, path, message, help_text in raw["issues"]
    ]

    routes: list[RouteInfo] = []
    for (
        path, methods, dependency_names, param_names, include_in_schema,
        has_response_model, response_model_str, status_code, tags,
        endpoint_name, has_docstring, source_ref,
    ) in raw["routes"]:
        source_file, _, line_str = source_ref.rpartition(":")
        routes.append(RouteInfo(
            path=path,
            methods=tuple(methods),
            dependency_names=frozenset(dependency_names),
            param_names=frozenset(param_names),
            include_in_schema=include_in_schema,
            has_response_model=has_response_model,
            response_model_str=response_model_str,
            status_code=status_code,
            tags=list(tags),
            endpoint_name=endpoint_name,
            has_docstring=has_docstring,
            source_file=source_file or source_ref,
            line=int(line_str) if line_str.isdigit() else 0,
        ))

    suppressions = [
        {"rule": rule, "reason": reason, "path": path, "line": line}
        for rule, reason, path, line in raw["suppressions"]
    ]

    _set_last_native_reason(raw.get("engine_reason", "native v2"))
    return {
        "issues": issues,
        "routes": routes,
        "suppressions": suppressions,
        "route_count": raw["route_count"],
        "openapi_path_count": None,
        "categories": raw["categories"],
        "score": raw["score"],
        "label": raw["label"],
        "checks_not_evaluated": raw.get("checks_not_evaluated", []),
    }


__all__ = [
    "get_native_rule_ids",
    "last_native_reason",
    "run_native_project_v2",
]
