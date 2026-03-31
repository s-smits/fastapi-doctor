from __future__ import annotations

"""Thin adapter over the Rust-native analysis engine."""

from typing import Any


class NativeEngineUnavailable(RuntimeError):
    """Raised when the compiled Rust extension is missing."""


def _load_native_module():
    try:
        from . import _fastapi_doctor_native
    except ImportError as exc:  # pragma: no cover - import-time failure
        raise NativeEngineUnavailable(
            "The Rust extension is unavailable. Reinstall the package with uv to build it."
        ) from exc
    return _fastapi_doctor_native


def get_native_rule_ids() -> frozenset[str]:
    """Return the set of rule IDs implemented by the Rust engine."""
    native_module = _load_native_module()
    return frozenset(native_module.get_all_rule_ids())


def _issue_tuple_to_dict(issue: list[Any] | tuple[Any, ...]) -> dict[str, Any]:
    check, severity, category, line, path, message, help_text = issue
    return {
        "check": check,
        "severity": severity,
        "category": category,
        "line": line,
        "path": path,
        "message": message,
        "help": help_text,
    }


def _route_tuple_to_dict(route: list[Any] | tuple[Any, ...]) -> dict[str, Any]:
    (
        path,
        methods,
        dependency_names,
        param_names,
        include_in_schema,
        has_response_model,
        response_model_str,
        status_code,
        tags,
        endpoint_name,
        has_docstring,
        source_ref,
    ) = route
    return {
        "path": path,
        "methods": list(methods),
        "dependency_names": list(dependency_names),
        "param_names": list(param_names),
        "include_in_schema": include_in_schema,
        "has_response_model": has_response_model,
        "response_model_str": response_model_str,
        "status_code": status_code,
        "tags": list(tags),
        "endpoint_name": endpoint_name,
        "has_docstring": has_docstring,
        "source_ref": source_ref,
    }


def _suppression_tuple_to_dict(suppression: list[Any] | tuple[Any, ...]) -> dict[str, Any]:
    rule, reason, path, line = suppression
    return {"rule": rule, "reason": reason, "path": path, "line": line}


def _coerce_native_result(raw: dict[str, Any]) -> dict[str, Any]:
    issues = raw.get("issues", [])
    if issues and isinstance(issues[0], dict):
        return {
            "issues": [dict(item) for item in issues],
            "routes": [dict(item) for item in raw.get("routes", [])],
            "suppressions": [dict(item) for item in raw.get("suppressions", [])],
            "route_count": int(raw.get("route_count", 0) or 0),
            "openapi_path_count": raw.get("openapi_path_count"),
            "categories": dict(raw.get("categories", {})),
            "score": int(raw.get("score", 100) or 100),
            "label": str(raw.get("label", "A")),
            "checks_not_evaluated": list(raw.get("checks_not_evaluated", [])),
            "engine_reason": str(raw.get("engine_reason", "rust-native")),
            "project_context": raw.get("project_context"),
        }

    return {
        "issues": [_issue_tuple_to_dict(item) for item in issues],
        "routes": [_route_tuple_to_dict(item) for item in raw.get("routes", [])],
        "suppressions": [
            _suppression_tuple_to_dict(item) for item in raw.get("suppressions", [])
        ],
        "route_count": int(raw.get("route_count", 0) or 0),
        "openapi_path_count": raw.get("openapi_path_count"),
        "categories": dict(raw.get("categories", {})),
        "score": int(raw.get("score", 100) or 100),
        "label": str(raw.get("label", "A")),
        "checks_not_evaluated": list(raw.get("checks_not_evaluated", [])),
        "engine_reason": str(raw.get("engine_reason", "rust-native")),
        "project_context": raw.get("project_context"),
    }


def analyze_selected_current_project_v2(
    *,
    profile: str | None,
    only_rules: list[str] | None,
    ignore_rules: list[str] | None,
    skip_structure: bool,
    skip_openapi: bool,
    static_only: bool = True,
    include_routes: bool = False,
) -> dict[str, Any]:
    """Run the current-project analysis through the Rust engine."""
    native_module = _load_native_module()
    raw = native_module.analyze_selected_current_project_v2(
        profile,
        only_rules,
        ignore_rules,
        skip_structure,
        skip_openapi,
        static_only,
        include_routes,
    )
    return _coerce_native_result(raw)


def score_current_project_v2(
    *,
    profile: str | None,
    only_rules: list[str] | None,
    ignore_rules: list[str] | None,
    skip_structure: bool,
    skip_openapi: bool,
    static_only: bool = True,
) -> int:
    """Return the current-project score from the Rust engine."""
    native_module = _load_native_module()
    return int(
        native_module.score_current_project_v2(
            profile,
            only_rules,
            ignore_rules,
            skip_structure,
            skip_openapi,
            static_only,
        )
    )


def get_rule_metadata() -> list[tuple[str, str, str]]:
    """Return all rule metadata as (rule_id, severity, category) tuples."""
    native_module = _load_native_module()
    return native_module.get_all_rule_metadata()


def get_project_context(*, static_only: bool = False) -> dict[str, Any]:
    """Return discovered project context from the Rust engine."""
    native_module = _load_native_module()
    raw = native_module.get_project_context(static_only)
    return dict(raw)


def get_scan_plan(
    *,
    profile: str | None = None,
    only_rules: list[str] | None = None,
    ignore_rules: list[str] | None = None,
    skip_structure: bool = False,
    skip_openapi: bool = False,
    static_only: bool = True,
) -> dict[str, Any]:
    """Return Rust-resolved scan planning data for the current project."""
    native_module = _load_native_module()
    raw = native_module.get_scan_plan(
        profile, only_rules, ignore_rules, skip_structure, skip_openapi, static_only
    )
    return dict(raw)


def get_profile_rule_ids(
    *,
    profile: str | None = None,
    only_rules: list[str] | None = None,
    ignore_rules: list[str] | None = None,
    skip_structure: bool = False,
    skip_openapi: bool = False,
) -> list[str]:
    """Return the resolved rule IDs for a given profile through the Rust engine."""
    native_module = _load_native_module()
    return native_module.get_profile_rule_ids(
        profile, only_rules, ignore_rules, skip_structure, skip_openapi
    )


__all__ = [
    "NativeEngineUnavailable",
    "analyze_selected_current_project_v2",
    "get_native_rule_ids",
    "get_project_context",
    "get_scan_plan",
    "get_profile_rule_ids",
    "get_rule_metadata",
    "score_current_project_v2",
]
