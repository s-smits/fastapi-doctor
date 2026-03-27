"""Lazy compatibility surface for legacy Python check imports."""

from __future__ import annotations

from importlib import import_module

_CHECK_MODULES = (
    "architecture",
    "configuration",
    "correctness",
    "performance",
    "pydantic",
    "resilience",
    "route_checks",
    "security",
)


def __getattr__(name: str):
    for module_name in _CHECK_MODULES:
        module = import_module(f"{__name__}.{module_name}")
        if hasattr(module, name):
            return getattr(module, name)
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")


def __dir__() -> list[str]:
    names = set(globals())
    for module_name in _CHECK_MODULES:
        module = import_module(f"{__name__}.{module_name}")
        names.update(getattr(module, "__all__", ()))
        names.update(name for name in vars(module) if name.startswith("check_"))
    return sorted(names)
