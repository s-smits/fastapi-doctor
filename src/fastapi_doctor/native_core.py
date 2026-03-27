from __future__ import annotations

"""Thin adapter over the PyO3 native static analysis engine."""

import os
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .models import DoctorIssue
    from .static_routes import RouteInfo

_LAST_NATIVE_REASON = "native not evaluated yet"
_NATIVE_ROUTE_RULES = frozenset({
    "security/forbidden-write-param",
    "correctness/duplicate-route",
    "correctness/missing-response-model",
    "correctness/post-status-code",
    "api-surface/missing-tags",
    "api-surface/missing-docstring",
    "api-surface/missing-pagination",
})


class NativeStaticModeUnavailable(RuntimeError):
    """Static analysis requires the native extension in the next major line."""


def _set_last_native_reason(reason: str) -> None:
    global _LAST_NATIVE_REASON
    _LAST_NATIVE_REASON = reason


def last_native_reason() -> str:
    return _LAST_NATIVE_REASON


def _native_enabled() -> bool:
    value = os.environ.get("FASTAPI_DOCTOR_DISABLE_NATIVE", "").strip().lower()
    return value not in {"1", "true", "yes", "on"}


def _static_native_error(reason: str) -> NativeStaticModeUnavailable:
    return NativeStaticModeUnavailable(
        f"Static analysis requires the native engine: {reason}"
    )


def _load_native_module(*, required_for_static: bool = False):
    if not _native_enabled():
        reason = "native disabled by FASTAPI_DOCTOR_DISABLE_NATIVE"
        _set_last_native_reason(reason)
        if required_for_static:
            raise _static_native_error(reason)
        return None

    try:
        from . import _fastapi_doctor_native
    except ImportError as e:
        reason = f"failed to import native PyO3 module: {e}"
        _set_last_native_reason(reason)
        if required_for_static:
            raise _static_native_error(reason)
        return None

    return _fastapi_doctor_native


def get_native_rule_ids() -> frozenset[str]:
    """Return the set of rule IDs owned by the Rust engine."""
    try:
        native_module = _load_native_module()
        if native_module is None:
            return frozenset()
        return frozenset(native_module.get_all_rule_ids())
    except Exception:
        return frozenset()


def score_native_project_auto_v2(
    *,
    profile: str | None,
    only_rules: list[str] | None,
    ignore_rules: list[str] | None,
    skip_structure: bool,
    skip_openapi: bool,
    static_only: bool = True,
) -> int | None:
    try:
        native_module = _load_native_module(required_for_static=True)
    except NativeStaticModeUnavailable:
        return None
    assert native_module is not None

    try:
        score = native_module.score_current_project_v2(
            profile,
            only_rules,
            ignore_rules,
            skip_structure,
            skip_openapi,
            static_only,
        )
    except Exception as e:
        _set_last_native_reason(f"native execution failed: {e}")
        return None

    _set_last_native_reason("native auto score v2")
    return int(score)


def run_native_selected_project_auto_v2(
    *,
    profile: str | None,
    only_rules: list[str] | None,
    ignore_rules: list[str] | None,
    skip_structure: bool,
    skip_openapi: bool,
    include_routes: bool = True,
    static_only: bool = True,
    require_native: bool = False,
) -> dict | None:
    try:
        native_module = _load_native_module(required_for_static=require_native)
    except NativeStaticModeUnavailable:
        raise
    if native_module is None:
        return None

    try:
        raw = native_module.analyze_selected_current_project_v2(
            profile,
            only_rules,
            ignore_rules,
            skip_structure,
            skip_openapi,
            static_only,
            include_routes,
        )
    except Exception as e:
        reason = f"native execution failed: {e}"
        _set_last_native_reason(reason)
        if require_native:
            raise _static_native_error(reason) from e
        return None

    return _coerce_native_project_result(raw, default_reason="native selected auto v2")


def _coerce_native_project_result(raw: dict, *, default_reason: str) -> dict:
    from .models import DoctorIssue
    from .static_routes import RouteInfo

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

    _set_last_native_reason(raw.get("engine_reason", default_reason))
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
        "project_context": raw.get("project_context"),
    }


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
    try:
        native_module = _load_native_module()
    except NativeStaticModeUnavailable:
        return None
    if native_module is None:
        return None

    from . import project
    include_routes = include_routes or bool(active_rules.intersection(_NATIVE_ROUTE_RULES))

    try:
        raw = native_module.analyze_project_v2(
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
            sorted(project.FORBIDDEN_WRITE_PARAMS),
            list(project.POST_CREATE_PREFIXES),
            list(project.TAG_REQUIRED_PREFIXES),
            sorted(active_rules),
            include_routes,
        )
    except Exception as e:
        _set_last_native_reason(f"native execution failed: {e}")
        return None

    return _coerce_native_project_result(raw, default_reason="native v2")


def run_native_project_auto_v2(
    active_rules: set[str],
    *,
    include_routes: bool = True,
    static_only: bool = True,
) -> dict | None:
    """Run native analysis with Rust-owned project context resolution."""
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
    try:
        native_module = _load_native_module()
    except NativeStaticModeUnavailable:
        return None
    if native_module is None:
        return None

    include_routes = include_routes or bool(active_rules.intersection(_NATIVE_ROUTE_RULES))

    try:
        raw = native_module.analyze_current_project_v2(
            sorted(active_rules),
            include_routes,
            static_only,
        )
    except Exception as e:
        _set_last_native_reason(f"native execution failed: {e}")
        return None

    return _coerce_native_project_result(raw, default_reason="native auto v2")


__all__ = [
    "NativeStaticModeUnavailable",
    "get_native_rule_ids",
    "last_native_reason",
    "run_native_selected_project_auto_v2",
    "score_native_project_auto_v2",
    "run_native_project_auto_v2",
    "run_native_project_v2",
]
