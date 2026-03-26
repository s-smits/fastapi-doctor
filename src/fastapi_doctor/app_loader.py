from __future__ import annotations

"""FastAPI app loading and route helper utilities."""

import os
from importlib.util import find_spec
from typing import Any

try:
    from fastapi import FastAPI
    from fastapi.routing import APIRoute
except ImportError:  # pragma: no cover
    FastAPI = Any  # type: ignore[assignment]
    APIRoute = Any  # type: ignore[assignment]

from . import project


def _ensure_env_defaults() -> None:
    for key, value in project.DEFAULT_ENV.items():
        os.environ.setdefault(key, value)


def fastapi_runtime_available() -> bool:
    return find_spec("fastapi") is not None


def build_app_for_doctor() -> FastAPI:
    _ensure_env_defaults()
    project.ensure_runtime_config()

    import sys
    import importlib

    if not project.APP_MODULE:
        raise RuntimeError(
            "Could not auto-detect a FastAPI app module. Pass --app-module or set DOCTOR_APP_MODULE."
        )

    module_path, app_name = project.APP_MODULE.split(":", 1)

    # Add the discovered import root so src/ and backend/ layouts import cleanly.
    if str(project.IMPORT_ROOT) not in sys.path:
        sys.path.insert(0, str(project.IMPORT_ROOT))

    module = importlib.import_module(module_path)
    if app_name.endswith("()"):
        factory = getattr(module, app_name[:-2])
        app = factory()
    else:
        app = getattr(module, app_name)

    return app


def iter_api_routes(app: FastAPI) -> list[APIRoute]:
    return [route for route in app.routes if isinstance(route, APIRoute)]


def dependency_names(route: APIRoute) -> set[str]:
    names: set[str] = set()
    for dependency in route.dependant.dependencies:
        call = dependency.call
        if call is None:
            continue
        names.add(getattr(call, "__name__", call.__class__.__name__))
    return names


def _route_matches_prefix(path: str, prefix: str) -> bool:
    return path == prefix or path.startswith(f"{prefix}/")


def _sorted_methods(route: APIRoute) -> tuple[str, ...]:
    return tuple(sorted(method for method in route.methods if method not in {"HEAD", "OPTIONS"}))

__all__ = [
    "FastAPI",
    "APIRoute",
    "_route_matches_prefix",
    "_sorted_methods",
    "build_app_for_doctor",
    "dependency_names",
    "fastapi_runtime_available",
    "iter_api_routes",
]
