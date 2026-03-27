from __future__ import annotations

"""Doctor orchestration and rule runner."""

from typing import TYPE_CHECKING, Any

from . import native_core, project
from .models import DoctorIssue, DoctorReport

if TYPE_CHECKING:
    from fastapi import FastAPI
    from .static_routes import RouteInfo
else:  # pragma: no cover
    FastAPI = Any  # type: ignore[assignment]


_SECURITY_SELECTORS = frozenset({
    "security/*",
    "pydantic/sensitive-field-type",
    "pydantic/extra-allow-on-request",
    "config/direct-env-access",
})
_MEDIUM_SELECTORS = _SECURITY_SELECTORS | frozenset({
    "correctness/*",
    "resilience/*",
    "config/*",
    "pydantic/mutable-default",
    "pydantic/deprecated-validator",
    "architecture/async-without-await",
    "architecture/avoid-sys-exit",
    "architecture/engine-pool-pre-ping",
    "architecture/missing-startup-validation",
    "architecture/passthrough-function",
    "architecture/print-in-production",
    "api-surface/missing-pagination",
    "api-surface/missing-operation-id",
    "api-surface/duplicate-operation-id",
    "api-surface/missing-openapi-tags",
})


def _selector_matches(rule_id: str, selector: str) -> bool:
    selector = selector.strip()
    if selector.endswith("*"):
        selector = selector[:-1]
    return rule_id == selector or rule_id.startswith(selector)


def _should_run(
    rule_id: str,
    only_rules: set[str] | None,
    ignore_rules: set[str] | None,
    profile: str | None,
) -> bool:
    if only_rules:
        return any(_selector_matches(rule_id, s) for s in only_rules)

    if profile:
        if profile == "security":
            if not any(_selector_matches(rule_id, s) for s in _SECURITY_SELECTORS):
                return False
        elif profile == "medium":
            if not any(_selector_matches(rule_id, s) for s in _MEDIUM_SELECTORS):
                return False

    if ignore_rules:
        return not any(_selector_matches(rule_id, s) for s in ignore_rules)

    if project.EXCLUDE_RULES:
        return not any(_selector_matches(rule_id, s) for s in project.EXCLUDE_RULES)

    return True


def run_python_doctor_checks(
    app: FastAPI | None = None,
    only_rules: set[str] | None = None,
    ignore_rules: set[str] | None = None,
    profile: str | None = None,
    skip_app_bootstrap: bool = False,
) -> DoctorReport:
    """Run all opinionated checks and compute a health score."""
    project.refresh_runtime_config(static_only=skip_app_bootstrap)
    libraries = project.discover_libraries()

    def should_run(rule_id: str) -> bool:
        return _should_run(rule_id, only_rules, ignore_rules, profile)

    all_rust_rules = native_core.get_native_rule_ids()
    active_rust_rules = {rule_id for rule_id in all_rust_rules if should_run(rule_id)}

    from .checks.route_checks import check_openapi_schema, check_route_dependency_policies

    route_list_checks = [
        ("security/missing-auth-dep", check_route_dependency_policies),
    ]
    native_route_rule_ids = {
        "security/forbidden-write-param",
        "correctness/duplicate-route",
        "api-surface/missing-tags",
        "correctness/missing-response-model",
        "correctness/post-status-code",
        "api-surface/missing-docstring",
        "api-surface/missing-pagination",
    }
    openapi_rule_ids = {
        "api-surface/missing-operation-id",
        "api-surface/duplicate-operation-id",
        "api-surface/missing-openapi-tags",
    }

    issues: list[DoctorIssue] = []
    checks_not_evaluated: list[str] = []
    routes: list[RouteInfo] = []
    route_count = 0
    openapi_path_count = 0
    live_app = None

    route_list_rule_ids = {rule_id for rule_id, _ in route_list_checks}
    needs_static_routes = any(should_run(rule_id) for rule_id in (route_list_rule_ids | native_route_rule_ids))

    if skip_app_bootstrap:
        checks_not_evaluated = sorted(rule_id for rule_id in openapi_rule_ids if should_run(rule_id))
    elif not libraries.fastapi:
        checks_not_evaluated = sorted(rule_id for rule_id in openapi_rule_ids if should_run(rule_id))
    else:
        from .app_loader import (
            build_app_for_doctor,
            fastapi_runtime_available,
            iter_api_routes,
        )
        from .static_routes import route_info_from_live_route

        if not fastapi_runtime_available():
            checks_not_evaluated = sorted(rule_id for rule_id in openapi_rule_ids if should_run(rule_id))
        else:
            try:
                live_app = app or build_app_for_doctor()
                live_routes = iter_api_routes(live_app)
                routes = [route_info_from_live_route(route) for route in live_routes]
                route_count = len(routes)
                openapi_path_count = len(live_app.openapi().get("paths", {}))
            except Exception as exc:
                import traceback

                issues.append(
                    DoctorIssue(
                        check="doctor/app-bootstrap-failed",
                        severity="error",
                        message=f"FastAPI app failed to boot — route-level checks were skipped: {exc}",
                        path=str(project.APP_MODULE or "unknown"),
                        category="Doctor",
                        help="Fix the import/startup error so route, auth, and OpenAPI checks can run.",
                        detail=traceback.format_exc(),
                    )
                )
                checks_not_evaluated = sorted(
                    rule_id
                    for rule_id in openapi_rule_ids
                    if should_run(rule_id)
                )

    native_result = native_core.run_native_project_v2(
        active_rust_rules,
        include_routes=needs_static_routes,
    )
    native_suppressions: list[dict] | None = None
    if native_result is not None:
        issues.extend(native_result["issues"])
        native_suppressions = native_result["suppressions"]
        if needs_static_routes and not routes:
            routes = native_result["routes"]
            route_count = native_result["route_count"]

    if routes:
        for rule_id, check_func in route_list_checks:
            if should_run(rule_id):
                issues.extend(check_func(routes))

    if live_app is not None and any(should_run(rule_id) for rule_id in openapi_rule_ids):
        issues.extend(check_openapi_schema(live_app))

    if only_rules:
        issues = [
            issue for issue in issues if any(_selector_matches(issue.check, rule_id) for rule_id in only_rules)
        ]
    elif ignore_rules:
        issues = [
            issue
            for issue in issues
            if not any(_selector_matches(issue.check, rule_id) for rule_id in ignore_rules)
        ]

    if native_suppressions is not None:
        all_suppressions = native_suppressions
    else:
        from .suppression import collect_suppressions

        all_suppressions: list[dict] = []
        for module in project.parsed_python_modules():
            all_suppressions.extend(collect_suppressions(module.source, module.rel_path))

    return DoctorReport(
        route_count=route_count,
        openapi_path_count=openapi_path_count,
        issues=issues,
        checks_not_evaluated=checks_not_evaluated,
        suppressions=all_suppressions,
    )


__all__ = ["run_python_doctor_checks"]
