from __future__ import annotations

"""Legacy Python route checks kept only for import compatibility."""

import ast

from .. import project
from ..models import DoctorIssue
from ..static_routes import RouteInfo


def check_write_route_parameters(routes: list[RouteInfo]) -> list[DoctorIssue]:
    """Write endpoints must not accept raw ownership fields — derive from auth."""
    issues: list[DoctorIssue] = []
    write_methods = {"POST", "PUT", "PATCH", "DELETE"}
    for route in routes:
        methods = set(route.methods)
        if not methods.intersection(write_methods):
            continue
        forbidden = sorted(project.FORBIDDEN_WRITE_PARAMS.intersection(route.param_names))
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


def check_duplicate_routes(routes: list[RouteInfo]) -> list[DoctorIssue]:
    """Duplicate route registrations cause silent shadowing."""
    seen: dict[tuple[str, str], RouteInfo] = {}
    issues: list[DoctorIssue] = []
    for route in routes:
        for method in route.methods:
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


def check_post_status_codes(routes: list[RouteInfo]) -> list[DoctorIssue]:
    """POST endpoints creating resources should return 201, not default 200."""
    issues: list[DoctorIssue] = []
    create_prefixes = project.POST_CREATE_PREFIXES
    if not create_prefixes:
        return issues
    mutation_suffixes = ("/undo", "/redo", "/restore", "/refresh", "/clone", "/fork")
    for route in routes:
        if "POST" not in route.methods:
            continue
        if route.status_code and route.status_code != 200:
            continue
        if any(route.path.endswith(s) for s in mutation_suffixes):
            continue
        if any(route.path.startswith(p) for p in create_prefixes):
            issues.append(
                DoctorIssue(
                    check="correctness/post-status-code",
                    severity="warning",
                    message="POST endpoint defaults to 200 — consider 201 for resource creation",
                    path=route.path,
                    category="Correctness",
                    help="Add status_code=201 to the route decorator.",
                    methods=route.methods,
                )
            )
    return issues


def check_response_models(routes: list[RouteInfo]) -> list[DoctorIssue]:
    """API routes in /api/ should use response_model for type safety and OpenAPI docs."""
    issues: list[DoctorIssue] = []
    exempt_patterns = ("/stream", "/export", "-export", "/download", "/webhook", "/oauth", "/callback", "/{")
    for route in routes:
        if not route.path.startswith("/api/"):
            continue
        if not route.include_in_schema:
            continue
        if any(p in route.path for p in exempt_patterns):
            continue
        if not route.has_response_model:
            issues.append(
                DoctorIssue(
                    check="correctness/missing-response-model",
                    severity="warning",
                    message="API endpoint has no response_model — weakens type safety and OpenAPI docs",
                    path=route.path,
                    category="Correctness",
                    help="Add response_model=YourPydanticModel to the route decorator.",
                    methods=route.methods,
                )
            )
    return issues


def check_route_tags(routes: list[RouteInfo]) -> list[DoctorIssue]:
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
                    methods=route.methods,
                )
            )
    return issues


def check_endpoint_docstrings(routes: list[RouteInfo]) -> list[DoctorIssue]:
    """Public API endpoints should have docstrings for generated docs."""
    issues: list[DoctorIssue] = []
    for route in routes:
        if not route.path.startswith("/api/"):
            continue
        if not route.include_in_schema:
            continue
        if not route.has_docstring:
            issues.append(
                DoctorIssue(
                    check="api-surface/missing-docstring",
                    severity="warning",
                    message=f"Endpoint '{route.endpoint_name}' has no docstring — weakens API docs",
                    path=route.path,
                    category="API Surface",
                    help="Add a docstring to the handler function. FastAPI uses it for OpenAPI descriptions.",
                    methods=route.methods,
                )
            )
    return issues


def check_missing_pagination(routes: list[RouteInfo]) -> list[DoctorIssue]:
    """List endpoints missing pagination (limit/offset) risk memory exhaustion."""
    issues: list[DoctorIssue] = []
    pagination_params = {"limit", "offset", "page", "per_page", "cursor"}
    exempt_patterns = ("/stream", "/export", "-export", "/download", "/webhook")
    for route in routes:
        if "GET" not in route.methods:
            continue
        if not route.path.startswith("/api/"):
            continue
        if any(p in route.path for p in exempt_patterns):
            continue
        if route.response_model_str and ("list[" in route.response_model_str or "paginated" in route.response_model_str):
            if not route.param_names.intersection(pagination_params):
                issues.append(
                    DoctorIssue(
                        check="api-surface/missing-pagination",
                        severity="warning",
                        message=f"List endpoint '{route.path}' has no pagination — risks memory exhaustion",
                        path=route.path,
                        category="API Surface",
                        help="Add limit and offset Query parameters to support pagination.",
                        methods=route.methods,
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
            is_route = any(
                isinstance(dec, (ast.Call, ast.Attribute, ast.Name))
                and ("router" in ast.dump(dec).lower() or "app" in ast.dump(dec).lower())
                for dec in node.decorator_list
            )
            if not is_route:
                continue
            func_len = (node.end_lineno or node.lineno) - node.lineno + 1
            if func_len > project._FAT_ROUTE_HANDLER_THRESHOLD:
                lines = module.source.splitlines()
                if node.lineno <= len(lines) and "# noqa: architecture" in lines[node.lineno - 1]:
                    continue
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


__all__ = [
    "check_write_route_parameters",
    "check_duplicate_routes",
    "check_post_status_codes",
    "check_response_models",
    "check_route_tags",
    "check_endpoint_docstrings",
    "check_missing_pagination",
    "check_fat_route_handlers",
]
