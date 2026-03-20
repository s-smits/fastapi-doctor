from __future__ import annotations

"""Route and OpenAPI focused checks."""

import ast
import inspect
from typing import Any

try:
    from fastapi import FastAPI
    from fastapi.routing import APIRoute
except ImportError:  # pragma: no cover
    FastAPI = Any  # type: ignore[assignment]
    APIRoute = Any  # type: ignore[assignment]

from .. import project
from ..app_loader import _route_matches_prefix, _sorted_methods, dependency_names
from ..models import DoctorIssue

def check_route_dependency_policies(routes: list[APIRoute]) -> list[DoctorIssue]:
    """Configured protected routes must carry the declared auth dependencies."""
    issues: list[DoctorIssue] = []
    for route in routes:
        path = route.path
        deps = dependency_names(route)
        methods = _sorted_methods(route)
        for prefix, rule_options in project.PROTECTED_ROUTE_RULES:
            if not _route_matches_prefix(path, prefix):
                continue
            if not any(required.issubset(deps) for required in rule_options):
                expected = " OR ".join(", ".join(sorted(required)) for required in rule_options)
                issues.append(
                    DoctorIssue(
                        check="security/missing-auth-dep",
                        severity="error",
                        message=f"Protected route is missing required dependencies: {expected}",
                        path=path,
                        category="Security",
                        help="Add the required Depends() to the route decorator or router.",
                        detail=f"Present dependencies: {', '.join(sorted(deps)) or '[none]'}",
                        methods=methods,
                    )
                )
            break
    return issues

def check_write_route_parameters(routes: list[APIRoute]) -> list[DoctorIssue]:
    """Write endpoints must not accept raw ownership fields — derive from auth."""
    issues: list[DoctorIssue] = []
    write_methods = {"POST", "PUT", "PATCH", "DELETE"}
    for route in routes:
        methods = set(_sorted_methods(route))
        if not methods.intersection(write_methods):
            continue
        params = inspect.signature(route.endpoint).parameters
        forbidden = sorted(project.FORBIDDEN_WRITE_PARAMS.intersection(params))
        if forbidden:
            issues.append(
                DoctorIssue(
                    check="security/forbidden-write-param",
                    severity="error",
                    message=f"Write endpoint accepts forbidden ownership parameters: {', '.join(forbidden)}",
                    path=route.path,
                    category="Security",
                    help="Derive identity from auth or request context dependencies instead.",
                    detail=f"Forbidden params: {', '.join(forbidden)}",
                    methods=tuple(sorted(methods)),
                )
            )
    return issues

def check_duplicate_routes(routes: list[APIRoute]) -> list[DoctorIssue]:
    """Duplicate route registrations cause silent shadowing."""
    seen: dict[tuple[str, str], APIRoute] = {}
    issues: list[DoctorIssue] = []
    for route in routes:
        for method in _sorted_methods(route):
            key = (method, route.path)
            if key in seen:
                issues.append(
                    DoctorIssue(
                        check="correctness/duplicate-route",
                        severity="error",
                        message=f"Duplicate route registration for {method} {route.path}",
                        path=route.path,
                        category="Correctness",
                        help="Remove the duplicate or use distinct paths.",
                        methods=(method,),
                    )
                )
            else:
                seen[key] = route
    return issues

def check_post_status_codes(routes: list[APIRoute]) -> list[DoctorIssue]:
    """POST endpoints creating resources should return 201, not default 200."""
    issues: list[DoctorIssue] = []
    create_prefixes = project.POST_CREATE_PREFIXES
    if not create_prefixes:
        return issues
    # Mutation endpoints that happen to match create prefixes but aren't creation
    mutation_suffixes = ("/undo", "/redo", "/restore", "/refresh", "/clone", "/fork")
    for route in routes:
        methods = _sorted_methods(route)
        if "POST" not in methods:
            continue
        if route.status_code and route.status_code != 200:
            continue  # Already set explicitly — fine
        # Skip mutation endpoints that aren't resource creation
        if any(route.path.endswith(s) for s in mutation_suffixes):
            continue
        # Only flag routes with clear "create" semantics
        if any(route.path.startswith(p) for p in create_prefixes):
            issues.append(
                DoctorIssue(
                    check="correctness/post-status-code",
                    severity="warning",
                    message="POST endpoint defaults to 200 — consider 201 for resource creation",
                    path=route.path,
                    category="Correctness",
                    help="Add status_code=201 to the route decorator.",
                    methods=methods,
                )
            )
    return issues

def check_response_models(routes: list[APIRoute]) -> list[DoctorIssue]:
    """API routes in /api/ should use response_model for type safety and OpenAPI docs."""
    issues: list[DoctorIssue] = []
    # Routes where raw responses are expected (streaming, files, redirects)
    exempt_patterns = ("/stream", "/export", "-export", "/download", "/webhook", "/oauth", "/callback", "/{")
    for route in routes:
        if not route.path.startswith("/api/"):
            continue
        if not route.include_in_schema:
            continue
        if any(p in route.path for p in exempt_patterns):
            continue
        if route.response_model is None:
            issues.append(
                DoctorIssue(
                    check="correctness/missing-response-model",
                    severity="warning",
                    message="API endpoint has no response_model — weakens type safety and OpenAPI docs",
                    path=route.path,
                    category="Correctness",
                    help="Add response_model=YourPydanticModel to the route decorator.",
                    methods=_sorted_methods(route),
                )
            )
    return issues

def check_route_tags(routes: list[APIRoute]) -> list[DoctorIssue]:
    """API routes should have tags for OpenAPI grouping."""
    issues: list[DoctorIssue] = []
    for route in routes:
        if not any(route.path.startswith(prefix) for prefix in project.TAG_REQUIRED_PREFIXES):
            continue
        if route.include_in_schema and not route.tags:
            issues.append(
                DoctorIssue(
                    check="api-surface/missing-tags",
                    severity="warning",
                    message="API route is missing tags",
                    path=route.path,
                    category="API Surface",
                    help="Add tags=['your-domain'] to the route decorator.",
                    methods=_sorted_methods(route),
                )
            )
    return issues

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

def check_endpoint_docstrings(routes: list[APIRoute]) -> list[DoctorIssue]:
    """Public API endpoints should have docstrings for generated docs."""
    issues: list[DoctorIssue] = []
    for route in routes:
        if not route.path.startswith("/api/"):
            continue
        if not route.include_in_schema:
            continue
        endpoint = route.endpoint
        if not inspect.getdoc(endpoint):
            issues.append(
                DoctorIssue(
                    check="api-surface/missing-docstring",
                    severity="warning",
                    message=f"Endpoint '{endpoint.__name__}' has no docstring — weakens API docs",
                    path=route.path,
                    category="API Surface",
                    help="Add a docstring to the handler function. FastAPI uses it for OpenAPI descriptions.",
                    methods=_sorted_methods(route),
                )
            )
    return issues

def check_fat_route_handlers() -> list[DoctorIssue]:
    """Detect route handlers with too much logic. Business logic belongs in services/."""
    issues: list[DoctorIssue] = []
    router_dir = project.OWN_CODE_DIR / "routers"
    if not router_dir.is_dir():
        return issues

    for module in project.parsed_python_modules():
        if not module.path.is_relative_to(router_dir):
            continue

        for node in ast.walk(module.tree):
            if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                continue

            # Check if it's a route (has decorators like @router.get)
            is_route = any(
                isinstance(dec, (ast.Call, ast.Attribute, ast.Name))
                and ("router" in ast.dump(dec).lower() or "app" in ast.dump(dec).lower())
                for dec in node.decorator_list
            )
            if not is_route:
                continue

            func_len = (node.end_lineno or node.lineno) - node.lineno + 1
            if func_len > project._FAT_ROUTE_HANDLER_THRESHOLD:
                # Check for # noqa: architecture
                lines = module.source.splitlines()
                if node.lineno <= len(lines) and "# noqa: architecture" in lines[node.lineno - 1]:
                    continue
                # Also check decorator lines
                dec_line = node.decorator_list[0].lineno if node.decorator_list else node.lineno
                if dec_line <= len(lines) and "# noqa: architecture" in lines[dec_line - 1]:
                    continue

                issues.append(
                    DoctorIssue(
                        check="architecture/fat-route-handler",
                        severity="warning",
                        message=f"Route handler '{node.name}' is {func_len} lines — extract business logic to services/",
                        path=module.rel_path,
                        category="Architecture",
                        help=f"Keep handlers under {project._FAT_ROUTE_HANDLER_THRESHOLD} lines. Move logic to a service function.",
                        line=node.lineno,
                    )
                )
    return issues

def check_missing_pagination(routes: list[APIRoute]) -> list[DoctorIssue]:
    """Detect list endpoints missing pagination (limit/offset).

    Endpoints returning lists should support pagination to avoid memory
    exhaustion and slow responses as datasets grow.
    """
    issues: list[DoctorIssue] = []
    pagination_params = {"limit", "offset", "page", "per_page", "cursor"}
    exempt_patterns = ("/stream", "/export", "-export", "/download", "/webhook")

    for route in routes:
        if "GET" not in _sorted_methods(route):
            continue
        if not route.path.startswith("/api/"):
            continue
        if any(p in route.path for p in exempt_patterns):
            continue

        # Check if the response model is a list or contains a list
        is_list_response = False
        res_model = route.response_model
        if res_model:
            model_str = str(res_model).lower()
            if "list[" in model_str or "paginated" in model_str:
                is_list_response = True

        if is_list_response:
            params = inspect.signature(route.endpoint).parameters
            has_pagination = any(p in params for p in pagination_params)
            if not has_pagination:
                issues.append(
                    DoctorIssue(
                        check="api-surface/missing-pagination",
                        severity="warning",
                        message=f"List endpoint '{route.path}' has no pagination — risks memory exhaustion",
                        path=route.path,
                        category="API Surface",
                        help="Add limit and offset Query parameters to support pagination.",
                        methods=_sorted_methods(route),
                    )
                )
    return issues


__all__ = [
    "check_route_dependency_policies",
    "check_write_route_parameters",
    "check_duplicate_routes",
    "check_post_status_codes",
    "check_response_models",
    "check_route_tags",
    "check_openapi_schema",
    "check_endpoint_docstrings",
    "check_fat_route_handlers",
    "check_missing_pagination",
]
