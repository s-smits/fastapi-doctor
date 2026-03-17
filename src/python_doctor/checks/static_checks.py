from __future__ import annotations

"""Compatibility shim for legacy static check imports."""

from .architecture import (
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
from .configuration import check_direct_env_access
from .correctness import (
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
from .performance import (
    check_n_plus_one_hint,
    check_regex_in_loop,
    check_sequential_awaits,
)
from .pydantic import (
    check_deprecated_validators,
    check_extra_allow_on_request_models,
    check_mutable_model_defaults,
    check_should_be_pydantic_model,
)
from .resilience import (
    check_bare_except_pass,
    check_broad_except_no_context,
    check_exception_swallowed_silently,
    check_reraise_without_context,
)
from .security import (
    check_assert_in_production,
    check_cors_wildcard,
    check_exception_detail_leak,
    check_hardcoded_secrets,
    check_shell_true,
    check_sql_fstring_interpolation,
    check_unsafe_hash_usage,
    check_unsafe_yaml_load,
)

__all__ = [
    "check_assert_in_production",
    "check_async_without_await",
    "check_asyncio_run_in_async_context",
    "check_avoid_os_path",
    "check_avoid_sys_exit",
    "check_bare_except_pass",
    "check_broad_except_no_context",
    "check_cors_wildcard",
    "check_deep_nesting",
    "check_deprecated_typing_imports",
    "check_deprecated_validators",
    "check_direct_env_access",
    "check_exception_detail_leak",
    "check_exception_swallowed_silently",
    "check_extra_allow_on_request_models",
    "check_get_with_side_effect",
    "check_giant_functions",
    "check_god_modules",
    "check_hardcoded_secrets",
    "check_import_bloat",
    "check_mutable_default_arg",
    "check_mutable_model_defaults",
    "check_n_plus_one_hint",
    "check_naive_datetime",
    "check_passthrough_functions",
    "check_print_statements",
    "check_regex_in_loop",
    "check_reraise_without_context",
    "check_return_in_finally",
    "check_sequential_awaits",
    "check_shell_true",
    "check_should_be_pydantic_model",
    "check_sql_fstring_interpolation",
    "check_star_import",
    "check_sync_io_in_async",
    "check_threading_lock_in_async",
    "check_unreachable_code",
    "check_unsafe_hash_usage",
    "check_unsafe_yaml_load",
]
