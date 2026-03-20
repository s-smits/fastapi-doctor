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
    check_engine_pool_pre_ping,
    check_giant_functions,
    check_god_modules,
    check_import_bloat,
    check_passthrough_functions,
    check_print_statements,
    check_star_import,
    check_startup_validation,
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
    check_missing_http_timeouts,
    check_mutable_default_arg,
    check_naive_datetime,
    check_return_in_finally,
    check_serverless_filesystem_writes,
    check_sync_io_in_async,
    check_threading_lock_in_async,
    check_unreachable_code,
)
from .checks.performance import (
    check_heavy_imports,
    check_n_plus_one_hint,
    check_regex_in_loop,
    check_sequential_awaits,
)
from .checks.pydantic import (
    check_deprecated_validators,
    check_extra_allow_on_request_models,
    check_mutable_model_defaults,
    check_sensitive_fields_in_models,
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
    check_missing_pagination,
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
    check_missing_security_headers,
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
    profile: str | None = None,
) -> DoctorReport:
    """Run all opinionated checks and compute a health score."""
    project.refresh_runtime_config()
    libraries = project.discover_libraries()
    # Pre-warm the parsed modules cache — all static checks share this.
    project.parsed_python_modules()
    issues: list[DoctorIssue] = []

    route_count = 0
    openapi_path_count = 0

    # ── Profile Rule Mapping ─────────────────────────────────────────────
    # Profile labels allow users to choose the audit intensity.
    security_rules = {
        "security/*",
        "pydantic/sensitive-field-type",
        "pydantic/extra-allow-on-request",
        "config/direct-env-access",
    }
    medium_rules = security_rules | {
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
    }
    # 'strict' includes everything (default behavior if no profile or only_rules set)

    def selector_matches(rule_id: str, selector: str) -> bool:
        selector = selector.strip()
        if selector.endswith("*"):
            selector = selector[:-1]
        return rule_id == selector or rule_id.startswith(selector)

    def should_run(rule_id: str) -> bool:
        # 1. If only_rules is set, it has highest precedence
        if only_rules:
            return any(selector_matches(rule_id, selector) for selector in only_rules)

        # 2. If profile is set, check if rule belongs to the profile
        if profile:
            target_rules = set()
            if profile == "security":
                target_rules = security_rules
            elif profile == "medium":
                target_rules = medium_rules
            elif profile == "strict":
                return True  # Strict includes everything

            if not any(selector_matches(rule_id, sel) for sel in target_rules):
                return False

        # 3. If ignore_rules is set, filter them out
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

            # Rules that require the 'routes' list
            route_level_checks = [
                ("security/missing-auth-dep", check_route_dependency_policies),
                ("security/forbidden-write-param", check_write_route_parameters),
                ("correctness/duplicate-route", check_duplicate_routes),
                ("api-surface/missing-tags", check_route_tags),
                ("correctness/missing-response-model", check_response_models),
                ("correctness/post-status-code", check_post_status_codes),
                ("api-surface/missing-docstring", check_endpoint_docstrings),
                ("api-surface/missing-pagination", check_missing_pagination),
            ]
            for rule_id, check_func in route_level_checks:
                if should_run(rule_id):
                    issues.extend(check_func(routes))

            # Rules that require the 'app' object
            if any(
                should_run(r)
                for r in (
                    "api-surface/missing-operation-id",
                    "api-surface/duplicate-operation-id",
                    "api-surface/missing-openapi-tags",
                )
            ):
                issues.extend(check_openapi_schema(app))

        except Exception as e:
            import traceback

            print(f"Failed to boot FastAPI app for route checks: {e}")
            traceback.print_exc()
            # Skip route checks if bootstrapping fails
            pass

    # ── Static Checks (No arguments) ──────────────────────────────────────────
    # Most checks are static AST-based and can be run without booting the app.
    # Grouped by enablement flag and library dependency.
    static_checks = [
        # Correctness
        ("correctness/sync-io-in-async", check_sync_io_in_async, libraries.fastapi),
        ("correctness/naive-datetime", check_naive_datetime, True),
        ("correctness/avoid-os-path", check_avoid_os_path, True),
        ("correctness/asyncio-run-in-async", check_asyncio_run_in_async_context, True),
        ("correctness/threading-lock-in-async", check_threading_lock_in_async, True),
        ("correctness/deprecated-typing-imports", check_deprecated_typing_imports, True),
        ("correctness/serverless-filesystem-write", check_serverless_filesystem_writes, True),
        ("correctness/missing-http-timeout", check_missing_http_timeouts, True),
        ("correctness/mutable-default-arg", check_mutable_default_arg, True),
        ("correctness/return-in-finally", check_return_in_finally, True),
        ("correctness/unreachable-code", check_unreachable_code, True),
        ("correctness/get-with-side-effect", check_get_with_side_effect, True),
        # Architecture
        ("architecture/giant-function", check_giant_functions, project.ARCHITECTURE_ENABLED),
        ("architecture/god-module", check_god_modules, project.ARCHITECTURE_ENABLED),
        ("architecture/deep-nesting", check_deep_nesting, project.ARCHITECTURE_ENABLED),
        ("architecture/import-bloat", check_import_bloat, project.ARCHITECTURE_ENABLED),
        ("architecture/passthrough-function", check_passthrough_functions, project.ARCHITECTURE_ENABLED),
        ("architecture/engine-pool-pre-ping", check_engine_pool_pre_ping, project.ARCHITECTURE_ENABLED),
        (
            "architecture/async-without-await",
            check_async_without_await,
            project.ARCHITECTURE_ENABLED and libraries.fastapi,
        ),
        ("architecture/print-in-production", check_print_statements, project.ARCHITECTURE_ENABLED),
        ("architecture/avoid-sys-exit", check_avoid_sys_exit, project.ARCHITECTURE_ENABLED),
        ("architecture/star-import", check_star_import, project.ARCHITECTURE_ENABLED),
        ("architecture/missing-startup-validation", check_startup_validation, project.ARCHITECTURE_ENABLED),
        ("architecture/fat-route-handler", check_fat_route_handlers, project.ARCHITECTURE_ENABLED),
        # Configuration
        ("config/direct-env-access", check_direct_env_access, True),
        ("config/alembic-target-metadata", check_alembic_target_metadata, libraries.alembic),
        ("config/alembic-autogenerate-scope", check_alembic_autogenerate_scope, libraries.alembic),
        ("config/alembic-empty-autogen-revision", check_alembic_empty_autogen_revision, libraries.alembic),
        ("config/sqlalchemy-naming-convention", check_sqlalchemy_naming_convention, libraries.alembic),
        # Security
        ("security/weak-hash-without-flag", check_unsafe_hash_usage, True),
        ("security/unsafe-yaml-load", check_unsafe_yaml_load, True),
        ("security/exception-detail-leak", check_exception_detail_leak, True),
        ("security/sql-fstring-interpolation", check_sql_fstring_interpolation, True),
        ("security/assert-in-production", check_assert_in_production, True),
        ("security/subprocess-shell-true", check_shell_true, True),
        ("security/missing-security-headers", check_missing_security_headers, True),
        ("security/hardcoded-secret", check_hardcoded_secrets, True),
        ("security/cors-wildcard", check_cors_wildcard, True),
        # Resilience
        ("resilience/bare-except-pass", check_bare_except_pass, True),
        ("resilience/reraise-without-context", check_reraise_without_context, True),
        ("resilience/exception-swallowed", check_exception_swallowed_silently, True),
        ("resilience/broad-except-no-context", check_broad_except_no_context, True),
        # Performance
        ("performance/heavy-imports", check_heavy_imports, True),
        ("performance/sequential-awaits", check_sequential_awaits, True),
        ("performance/regex-in-loop", check_regex_in_loop, True),
        ("performance/n-plus-one-hint", check_n_plus_one_hint, True),
        # Pydantic
        ("pydantic/deprecated-validator", check_deprecated_validators, libraries.pydantic or libraries.fastapi),
        ("pydantic/mutable-default", check_mutable_model_defaults, libraries.pydantic or libraries.fastapi),
        ("pydantic/extra-allow-on-request", check_extra_allow_on_request_models, libraries.pydantic or libraries.fastapi),
        ("pydantic/should-be-model", check_should_be_pydantic_model, libraries.pydantic or libraries.fastapi),
        ("pydantic/sensitive-field-type", check_sensitive_fields_in_models, libraries.pydantic or libraries.fastapi),
    ]

    for rule_id, check_func, enabled in static_checks:
        if enabled and should_run(rule_id):
            issues.extend(check_func())

    # Final filtering of issues if prefix-based should_run wasn't granular enough
    if only_rules:
        issues = [issue for issue in issues if any(selector_matches(issue.check, selector) for selector in only_rules)]
    elif ignore_rules:
        issues = [
            issue for issue in issues if not any(selector_matches(issue.check, selector) for selector in ignore_rules)
        ]

    return DoctorReport(
        route_count=route_count,
        openapi_path_count=openapi_path_count,
        issues=issues,
    )

__all__ = ["run_python_doctor_checks"]
