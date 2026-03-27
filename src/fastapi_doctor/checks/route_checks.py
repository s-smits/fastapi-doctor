from __future__ import annotations

"""Active Python route checks plus lazy compatibility shims."""

from importlib import import_module
from typing import TYPE_CHECKING, Any

from .. import project
from ..models import DoctorIssue
from ..static_routes import RouteInfo

if TYPE_CHECKING:
    from fastapi import FastAPI
else:  # pragma: no cover
    FastAPI = Any  # type: ignore[assignment]

_LEGACY_EXPORTS = frozenset({
    "check_write_route_parameters",
    "check_duplicate_routes",
    "check_post_status_codes",
    "check_response_models",
    "check_route_tags",
    "check_endpoint_docstrings",
    "check_missing_pagination",
    "check_fat_route_handlers",
})


# ---------------------------------------------------------------------------
# Route-list checks (work with RouteInfo from either source)
# ---------------------------------------------------------------------------

def check_route_dependency_policies(routes: list[RouteInfo]) -> list[DoctorIssue]:
    """Configured protected routes must carry the declared auth dependencies."""
    issues: list[DoctorIssue] = []
    for route in routes:
        path = route.path
        deps = route.dependency_names
        for prefix, rule_options in project.PROTECTED_ROUTE_RULES:
            if not (path == prefix or path.startswith(f"{prefix}/")):
                continue
            if not any(required.issubset(deps) for required in rule_options):
                expected = " OR ".join(", ".join(sorted(req)) for req in rule_options)
                issues.append(
                    DoctorIssue(
                        check="security/missing-auth-dep",
                        severity="error",
                        message=f"Protected route is missing required dependencies: {expected}",
                        path=path,
                        category="Security",
                        help="Add the required Depends() to the route decorator or router.",
                        detail=f"Present dependencies: {', '.join(sorted(deps)) or '[none]'}",
                        methods=route.methods,
                    )
                )
            break
    return issues


# ---------------------------------------------------------------------------
# OpenAPI schema check (live app only — not available in static mode)
# ---------------------------------------------------------------------------

def check_openapi_schema(app: FastAPI) -> list[DoctorIssue]:
    """Validate OpenAPI schema quality: unique operation IDs, tags present."""
    schema = app.openapi()
    issues: list[DoctorIssue] = []
    seen_operation_ids: dict[str, tuple[str, str]] = {}
    for path, operations in schema.get("paths", {}).items():
        for method, operation in operations.items():
            op_id = operation.get("operationId")
            tags = operation.get("tags") or []
            if not op_id:
                issues.append(
                    DoctorIssue(
                        check="api-surface/missing-operation-id",
                        severity="error",
                        message="OpenAPI operation is missing operationId",
                        path=path,
                        category="API Surface",
                        help="FastAPI auto-generates operationId from function names — check the endpoint exists.",
                        methods=(method.upper(),),
                    )
                )
            elif op_id in seen_operation_ids:
                prior_path, prior_method = seen_operation_ids[op_id]
                issues.append(
                    DoctorIssue(
                        check="api-surface/duplicate-operation-id",
                        severity="error",
                        message=f"Duplicate OpenAPI operationId '{op_id}'",
                        path=path,
                        category="API Surface",
                        help="Rename one of the endpoint functions to make operationIds unique.",
                        detail=f"Also used by {prior_method} {prior_path}",
                        methods=(method.upper(),),
                    )
                )
            else:
                seen_operation_ids[op_id] = (path, method.upper())

            if any(path.startswith(prefix) for prefix in project.TAG_REQUIRED_PREFIXES) and not tags:
                issues.append(
                    DoctorIssue(
                        check="api-surface/missing-openapi-tags",
                        severity="warning",
                        message="OpenAPI operation is missing tags",
                        path=path,
                        category="API Surface",
                        help="Add tags to the route decorator for better API documentation.",
                        methods=(method.upper(),),
                    )
                )
    return issues


def __getattr__(name: str) -> Any:
    if name in _LEGACY_EXPORTS:
        module = import_module(".checks._legacy_route_checks", "fastapi_doctor")
        return getattr(module, name)
    raise AttributeError(name)


def __dir__() -> list[str]:
    return sorted(set(globals()) | _LEGACY_EXPORTS)


__all__ = [
    "check_route_dependency_policies",
    "check_openapi_schema",
    "check_write_route_parameters",
    "check_duplicate_routes",
    "check_post_status_codes",
    "check_response_models",
    "check_route_tags",
    "check_endpoint_docstrings",
    "check_missing_pagination",
    "check_fat_route_handlers",
]
