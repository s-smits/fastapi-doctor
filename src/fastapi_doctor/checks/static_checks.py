"""Lazy compatibility shim for legacy static check imports."""

from __future__ import annotations

from importlib import import_module

_STATIC_CHECK_MODULES = (
    "architecture",
    "configuration",
    "correctness",
    "performance",
    "pydantic",
    "resilience",
    "security",
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


def __getattr__(name: str):
    if name not in __all__:
        raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
    for module_name in _STATIC_CHECK_MODULES:
        module = import_module(f"{__package__}.{module_name}")
        if hasattr(module, name):
            return getattr(module, name)
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")


def __dir__() -> list[str]:
    return sorted(set(globals()) | set(__all__))
