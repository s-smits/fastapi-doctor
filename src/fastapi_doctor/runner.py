from __future__ import annotations

"""Doctor orchestration and rule runner."""

from typing import Any

try:
    from fastapi import FastAPI
except ImportError:  # pragma: no cover
    FastAPI = Any  # type: ignore[assignment]

from . import project
from .app_loader import build_app_for_doctor, iter_api_routes
from .checks.architecture import (
    check_async_without_await,
    check_avoid_sys_exit,
    check_deep_nesting,
    check_giant_functions,
    check_god_modules,
    check_import_bloat,
    check_passthrough_functions,
    check_print_statements,
    check_star_import,
)
from .checks.configuration import (
    check_alembic_autogenerate_scope,
    check_alembic_empty_autogen_revision,
    check_alembic_target_metadata,
    check_direct_env_access,
    check_sqlalchemy_naming_convention,
)
from .checks.correctness import (
    check_asyncio_run_in_async_context,
    check_avoid_os_path,
    check_deprecated_typing_imports,
    check_get_with_side_effect,
    check_mutable_default_arg,
    check_naive_datetime,
    check_return_in_finally,
    check_sync_io_in_async,
    check_threading_lock_in_async,
    check_unreachable_code,
)
from .checks.performance import (
    check_n_plus_one_hint,
    check_regex_in_loop,
    check_sequential_awaits,
)
from .checks.pydantic import (
    check_deprecated_validators,
    check_extra_allow_on_request_models,
    check_mutable_model_defaults,
    check_should_be_pydantic_model,
)
from .checks.resilience import (
    check_bare_except_pass,
    check_broad_except_no_context,
    check_exception_swallowed_silently,
    check_reraise_without_context,
)
from .checks.route_checks import (
    check_duplicate_routes,
    check_endpoint_docstrings,
    check_fat_route_handlers,
    check_openapi_schema,
    check_post_status_codes,
    check_response_models,
    check_route_dependency_policies,
    check_route_tags,
    check_write_route_parameters,
)
from .checks.security import (
    check_assert_in_production,
    check_cors_wildcard,
    check_exception_detail_leak,
    check_hardcoded_secrets,
    check_shell_true,
    check_sql_fstring_interpolation,
    check_unsafe_hash_usage,
    check_unsafe_yaml_load,
)
from .models import DoctorIssue, DoctorReport

def run_python_doctor_checks(
    app: FastAPI | None = None,
    only_rules: set[str] | None = None,
    ignore_rules: set[str] | None = None,
) -> DoctorReport:
    """Run all opinionated checks and compute a health score."""
    project.refresh_runtime_config()
    libraries = project.discover_libraries()
    # Pre-warm the parsed modules cache — all static checks share this.
    project.parsed_python_modules()
    issues: list[DoctorIssue] = []

    route_count = 0
    openapi_path_count = 0

    def selector_matches(rule_id: str, selector: str) -> bool:
        selector = selector.strip()
        if selector.endswith("*"):
            selector = selector[:-1]
        return rule_id == selector or rule_id.startswith(selector)

    def should_run(rule_id: str) -> bool:
        if only_rules:
            return any(selector_matches(rule_id, selector) for selector in only_rules)
        if ignore_rules:
            return not any(selector_matches(rule_id, selector) for selector in ignore_rules)
        return True

    # ── FastAPI route-level checks (need live app) ────────────────────────────────
    if libraries.fastapi:
        try:
            app = app or build_app_for_doctor()
            routes = iter_api_routes(app)
            route_count = len(routes)
            openapi_path_count = len(app.openapi().get("paths", {}))

            if should_run("security/missing-auth-dep"):
                issues.extend(check_route_dependency_policies(routes))
            if should_run("security/forbidden-write-param"):
                issues.extend(check_write_route_parameters(routes))
            if should_run("correctness/duplicate-route"):
                issues.extend(check_duplicate_routes(routes))
            if should_run("api-surface/missing-tags"):
                issues.extend(check_route_tags(routes))
            if any(
                should_run(rule_id)
                for rule_id in (
                    "api-surface/missing-operation-id",
                    "api-surface/duplicate-operation-id",
                    "api-surface/missing-openapi-tags",
                )
            ):
                issues.extend(check_openapi_schema(app))
            if should_run("correctness/missing-response-model"):
                issues.extend(check_response_models(routes))
            if should_run("correctness/post-status-code"):
                issues.extend(check_post_status_codes(routes))
            if should_run("api-surface/missing-docstring"):
                issues.extend(check_endpoint_docstrings(routes))
            if project.ARCHITECTURE_ENABLED and should_run("architecture/fat-route-handler"):
                issues.extend(check_fat_route_handlers())
        except Exception as e:
            import traceback
            print(f"Failed to boot FastAPI app for route checks: {e}")
            traceback.print_exc()
            # Skip route checks if bootstrapping fails
            pass

    # ── Correctness checks (static) ─────────────────────────────────────
    if libraries.fastapi and should_run("correctness/sync-io-in-async"):
        issues.extend(check_sync_io_in_async())
    if should_run("correctness/naive-datetime"):
        issues.extend(check_naive_datetime())

    # ── Architecture checks (AST / file scanning) ────────────────────────
    # Skippable via .fastapi-doctor.yml → architecture.enabled: false
    # Individual thresholds can also be set to 0 to disable specific rules.
    # Ruff covers many of these (C901, PLR0915, etc.) — defer to ruff if preferred.
    if not project.ARCHITECTURE_ENABLED:
        pass  # All architecture rules skipped via config
    elif should_run("architecture/giant-function"):
        issues.extend(check_giant_functions())
    if project.ARCHITECTURE_ENABLED and should_run("architecture/god-module"):
        issues.extend(check_god_modules())
    if project.ARCHITECTURE_ENABLED and should_run("architecture/deep-nesting"):
        issues.extend(check_deep_nesting())
    if project.ARCHITECTURE_ENABLED and should_run("architecture/import-bloat"):
        issues.extend(check_import_bloat())
    if project.ARCHITECTURE_ENABLED and should_run("architecture/passthrough-function"):
        issues.extend(check_passthrough_functions())
    if project.ARCHITECTURE_ENABLED and libraries.fastapi and should_run("architecture/async-without-await"):
        issues.extend(check_async_without_await())
    if project.ARCHITECTURE_ENABLED and should_run("architecture/print-in-production"):
        issues.extend(check_print_statements())
    if should_run("security/weak-hash-without-flag"):
        issues.extend(check_unsafe_hash_usage())
    if should_run("security/unsafe-yaml-load"):
        issues.extend(check_unsafe_yaml_load())
    if should_run("config/direct-env-access"):
        issues.extend(check_direct_env_access())
    if libraries.alembic and should_run("config/alembic-target-metadata"):
        issues.extend(check_alembic_target_metadata())
    if libraries.alembic and should_run("config/alembic-autogenerate-scope"):
        issues.extend(check_alembic_autogenerate_scope())
    if libraries.alembic and should_run("config/alembic-empty-autogen-revision"):
        issues.extend(check_alembic_empty_autogen_revision())
    if libraries.alembic and should_run("config/sqlalchemy-naming-convention"):
        issues.extend(check_sqlalchemy_naming_convention())
    if project.ARCHITECTURE_ENABLED and should_run("architecture/avoid-sys-exit"):
        issues.extend(check_avoid_sys_exit())
    # architecture/todo-in-production — removed: TODOs/FIXMEs are planning
    # artifacts, not bugs.  Ruff's FIX/TD rules cover this if desired.

    # ── Correctness checks (static) ───────────────────────────────────────────
    if should_run("correctness/avoid-os-path"):
        issues.extend(check_avoid_os_path())
    if should_run("correctness/asyncio-run-in-async"):
        issues.extend(check_asyncio_run_in_async_context())
    if should_run("correctness/threading-lock-in-async"):
        issues.extend(check_threading_lock_in_async())
    if should_run("correctness/deprecated-typing-imports"):
        issues.extend(check_deprecated_typing_imports())

    # ── Security checks (static) ───────────────────────────────────────────
    if should_run("security/exception-detail-leak"):
        issues.extend(check_exception_detail_leak())
    if should_run("security/sql-fstring-interpolation"):
        issues.extend(check_sql_fstring_interpolation())
    if should_run("security/assert-in-production"):
        issues.extend(check_assert_in_production())
    if should_run("security/subprocess-shell-true"):
        issues.extend(check_shell_true())

    # ── Resilience checks ────────────────────────────────────────────────
    if should_run("resilience/bare-except-pass"):
        issues.extend(check_bare_except_pass())
    if should_run("resilience/reraise-without-context"):
        issues.extend(check_reraise_without_context())
    if should_run("resilience/exception-swallowed"):
        issues.extend(check_exception_swallowed_silently())
    if should_run("resilience/broad-except-no-context"):
        issues.extend(check_broad_except_no_context())

    # ── Performance checks (inspired by react-doctor) ──────────────────
    if should_run("performance/sequential-awaits"):
        issues.extend(check_sequential_awaits())
    if should_run("performance/regex-in-loop"):
        issues.extend(check_regex_in_loop())
    if should_run("performance/n-plus-one-hint"):
        issues.extend(check_n_plus_one_hint())

    # ── Security checks (additional) ─────────────────────────────────────
    if should_run("security/hardcoded-secret"):
        issues.extend(check_hardcoded_secrets())
    if should_run("security/cors-wildcard"):
        issues.extend(check_cors_wildcard())

    # ── Correctness checks (additional) ──────────────────────────────────
    if should_run("correctness/mutable-default-arg"):
        issues.extend(check_mutable_default_arg())
    if should_run("correctness/return-in-finally"):
        issues.extend(check_return_in_finally())
    if should_run("correctness/unreachable-code"):
        issues.extend(check_unreachable_code())
    if should_run("correctness/get-with-side-effect"):
        issues.extend(check_get_with_side_effect())

    # ── Architecture checks (additional) ──────────────────────────────────
    if project.ARCHITECTURE_ENABLED and should_run("architecture/star-import"):
        issues.extend(check_star_import())

    # ── Pydantic checks ──────────────────────────────────────────────────
    if libraries.pydantic or libraries.fastapi:
        if should_run("pydantic/deprecated-validator"):
            issues.extend(check_deprecated_validators())
        if should_run("pydantic/mutable-default"):
            issues.extend(check_mutable_model_defaults())
        if should_run("pydantic/extra-allow-on-request"):
            issues.extend(check_extra_allow_on_request_models())
        if should_run("pydantic/should-be-model"):
            issues.extend(check_should_be_pydantic_model())

    # Final filtering of issues if prefix-based should_run wasn't granular enough
    if only_rules:
        issues = [issue for issue in issues if any(selector_matches(issue.check, selector) for selector in only_rules)]
    elif ignore_rules:
        issues = [issue for issue in issues if not any(selector_matches(issue.check, selector) for selector in ignore_rules)]

    return DoctorReport(
        route_count=route_count,
        openapi_path_count=openapi_path_count,
        issues=issues,
    )

__all__ = ["run_python_doctor_checks"]
