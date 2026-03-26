from __future__ import annotations

"""Static AST-based route extraction — knows FastAPI components without booting the app."""

import ast
import inspect
from dataclasses import dataclass, field
from typing import Any

from . import project

_HTTP_METHODS = frozenset({"get", "post", "put", "patch", "delete", "head", "options", "trace"})


# ---------------------------------------------------------------------------
# RouteInfo — unified representation for live and static routes
# ---------------------------------------------------------------------------

@dataclass(slots=True)
class RouteInfo:
    """Unified route representation consumed by all route checks."""

    path: str
    methods: tuple[str, ...]
    dependency_names: frozenset[str] = frozenset()
    param_names: frozenset[str] = frozenset()
    include_in_schema: bool = True
    has_response_model: bool = False
    response_model_str: str | None = None  # lowercased
    status_code: int | None = None
    tags: list[str] = field(default_factory=list)
    endpoint_name: str = ""
    has_docstring: bool = False
    source_file: str = ""
    line: int = 0


def route_info_from_live_route(route: Any) -> RouteInfo:
    """Adapt a live FastAPI ``APIRoute`` into a ``RouteInfo``."""
    methods = tuple(sorted(m for m in route.methods if m not in {"HEAD", "OPTIONS"}))
    dep_names: set[str] = set()
    for dep in route.dependant.dependencies:
        call = dep.call
        if call is not None:
            dep_names.add(getattr(call, "__name__", call.__class__.__name__))
    params = frozenset(inspect.signature(route.endpoint).parameters.keys())
    response_model_str = str(route.response_model).lower() if route.response_model is not None else None
    return RouteInfo(
        path=route.path,
        methods=methods,
        dependency_names=frozenset(dep_names),
        param_names=params,
        include_in_schema=route.include_in_schema,
        has_response_model=route.response_model is not None,
        response_model_str=response_model_str,
        status_code=route.status_code,
        tags=list(route.tags) if route.tags else [],
        endpoint_name=route.endpoint.__name__,
        has_docstring=bool(inspect.getdoc(route.endpoint)),
    )


# ---------------------------------------------------------------------------
# AST helpers
# ---------------------------------------------------------------------------

def _str(node: ast.expr) -> str | None:
    return node.value if isinstance(node, ast.Constant) and isinstance(node.value, str) else None

def _int(node: ast.expr) -> int | None:
    return node.value if isinstance(node, ast.Constant) and isinstance(node.value, int) else None

def _bool(node: ast.expr) -> bool | None:
    return node.value if isinstance(node, ast.Constant) and isinstance(node.value, bool) else None

def _str_list(node: ast.expr) -> list[str]:
    if isinstance(node, (ast.List, ast.Tuple)):
        return [s for elt in node.elts if (s := _str(elt)) is not None]
    return []

def _kw(keywords: list[ast.keyword], name: str) -> ast.expr | None:
    for kw in keywords:
        if kw.arg == name:
            return kw.value
    return None

_EMPTY_STR = ast.Constant(value="")
_EMPTY_LIST = ast.List(elts=[])


# ---------------------------------------------------------------------------
# Depends() extraction
# ---------------------------------------------------------------------------

def _is_depends(node: ast.expr) -> bool:
    if not isinstance(node, ast.Call):
        return False
    f = node.func
    return (isinstance(f, ast.Name) and f.id == "Depends") or (isinstance(f, ast.Attribute) and f.attr == "Depends")

def _dep_name(node: ast.Call) -> str | None:
    if node.args:
        a = node.args[0]
        if isinstance(a, ast.Name):
            return a.id
        if isinstance(a, ast.Attribute):
            return a.attr
    return None

def _func_deps(fdef: ast.FunctionDef | ast.AsyncFunctionDef) -> set[str]:
    names: set[str] = set()
    for d in fdef.args.defaults:
        if _is_depends(d) and (n := _dep_name(d)):
            names.add(n)
    for d in fdef.args.kw_defaults:
        if d is not None and _is_depends(d) and (n := _dep_name(d)):
            names.add(n)
    return names

def _decorator_deps(call: ast.Call) -> set[str]:
    deps_node = _kw(call.keywords, "dependencies")
    if deps_node is None:
        return set()
    names: set[str] = set()
    if isinstance(deps_node, (ast.List, ast.Tuple)):
        for elt in deps_node.elts:
            if _is_depends(elt) and (n := _dep_name(elt)):
                names.add(n)
    return names

def _param_names(fdef: ast.FunctionDef | ast.AsyncFunctionDef) -> frozenset[str]:
    names: set[str] = set()
    for a in fdef.args.args:
        if a.arg != "self":
            names.add(a.arg)
    for a in fdef.args.kwonlyargs:
        names.add(a.arg)
    return frozenset(names)

def _has_docstring(fdef: ast.FunctionDef | ast.AsyncFunctionDef) -> bool:
    if fdef.body:
        first = fdef.body[0]
        return isinstance(first, ast.Expr) and isinstance(first.value, ast.Constant) and isinstance(first.value.value, str)
    return False


# ---------------------------------------------------------------------------
# Router / include_router scanning
# ---------------------------------------------------------------------------

@dataclass
class _RouterMeta:
    prefix: str = ""
    tags: list[str] = field(default_factory=list)


def _scan_infrastructure(modules: list[project.ParsedModule]):
    """Find router variables and include_router() calls across all modules."""
    file_routers: dict[str, dict[str, _RouterMeta]] = {}
    # router_var_name -> (include_prefix, include_tags) from include_router() calls
    prefix_map: dict[str, tuple[str, list[str]]] = {}

    for mod in modules:
        local: dict[str, _RouterMeta] = {}
        for node in ast.walk(mod.tree):
            # APIRouter() / FastAPI() assignments
            if isinstance(node, ast.Assign) and isinstance(node.value, ast.Call):
                fname = None
                f = node.value.func
                if isinstance(f, ast.Name):
                    fname = f.id
                elif isinstance(f, ast.Attribute):
                    fname = f.attr
                if fname in ("APIRouter", "FastAPI"):
                    for target in node.targets:
                        if isinstance(target, ast.Name):
                            prefix = _str(_kw(node.value.keywords, "prefix") or _EMPTY_STR) or ""
                            tags = _str_list(_kw(node.value.keywords, "tags") or _EMPTY_LIST)
                            local[target.id] = _RouterMeta(prefix=prefix, tags=tags)

            # app.include_router(router, prefix=..., tags=...)
            if (
                isinstance(node, ast.Expr)
                and isinstance(node.value, ast.Call)
                and isinstance(node.value.func, ast.Attribute)
                and node.value.func.attr == "include_router"
                and node.value.args
            ):
                call = node.value
                rarg = call.args[0]
                rname = None
                if isinstance(rarg, ast.Name):
                    rname = rarg.id
                elif isinstance(rarg, ast.Attribute):
                    rname = rarg.attr
                if rname:
                    inc_prefix = _str(_kw(call.keywords, "prefix") or _EMPTY_STR) or ""
                    inc_tags = _str_list(_kw(call.keywords, "tags") or _EMPTY_LIST)
                    existing = prefix_map.get(rname)
                    if existing is None or len(inc_prefix) > len(existing[0]):
                        prefix_map[rname] = (inc_prefix, inc_tags)

        file_routers[mod.rel_path] = local

    return file_routers, prefix_map


# ---------------------------------------------------------------------------
# Public: extract_static_routes
# ---------------------------------------------------------------------------

def extract_static_routes() -> tuple[list[RouteInfo], int]:
    """Extract route metadata from AST. Returns ``(routes, count)``."""
    modules = project.parsed_python_modules()
    file_routers, prefix_map = _scan_infrastructure(modules)

    routes: list[RouteInfo] = []
    for mod in modules:
        local = file_routers.get(mod.rel_path, {})
        for node in ast.walk(mod.tree):
            if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                continue
            for dec in node.decorator_list:
                info = _parse_decorator(dec, node, mod, local, prefix_map)
                if info is not None:
                    routes.append(info)
                    break  # one route per function
    return routes, len(routes)


def _parse_decorator(
    dec: ast.expr,
    fdef: ast.FunctionDef | ast.AsyncFunctionDef,
    mod: project.ParsedModule,
    local_routers: dict[str, _RouterMeta],
    prefix_map: dict[str, tuple[str, list[str]]],
) -> RouteInfo | None:
    if not isinstance(dec, ast.Call) or not isinstance(dec.func, ast.Attribute):
        return None
    method_name = dec.func.attr.lower()

    if method_name in _HTTP_METHODS:
        methods: tuple[str, ...] = (method_name.upper(),)
    elif method_name == "api_route":
        mnode = _kw(dec.keywords, "methods")
        methods = tuple(s.upper() for s in _str_list(mnode)) if mnode else ("GET",)
    else:
        return None

    methods = tuple(m for m in methods if m not in ("HEAD", "OPTIONS"))
    if not methods:
        return None

    # Router variable for prefix resolution
    rvar: str | None = None
    if isinstance(dec.func.value, ast.Name):
        rvar = dec.func.value.id

    path = _str(dec.args[0]) if dec.args else ""
    path = path or ""

    # Combine prefixes: include_router prefix + router definition prefix + decorator path
    prefix = ""
    inherited_tags: list[str] = []
    if rvar:
        if rvar in local_routers:
            prefix = local_routers[rvar].prefix
            inherited_tags = list(local_routers[rvar].tags)
        if rvar in prefix_map:
            inc_prefix, inc_tags = prefix_map[rvar]
            prefix = inc_prefix + prefix
            inherited_tags = inc_tags + inherited_tags
    full_path = prefix + path

    resp_node = _kw(dec.keywords, "response_model")
    sc_node = _kw(dec.keywords, "status_code")
    tags_node = _kw(dec.keywords, "tags")
    iis_node = _kw(dec.keywords, "include_in_schema")

    include_in_schema = True
    if iis_node is not None:
        val = _bool(iis_node)
        if val is not None:
            include_in_schema = val

    return RouteInfo(
        path=full_path,
        methods=methods,
        dependency_names=frozenset(_func_deps(fdef) | _decorator_deps(dec)),
        param_names=_param_names(fdef),
        include_in_schema=include_in_schema,
        has_response_model=resp_node is not None,
        response_model_str=ast.unparse(resp_node).lower() if resp_node else None,
        status_code=_int(sc_node) if sc_node else None,
        tags=_str_list(tags_node) if tags_node else inherited_tags,
        endpoint_name=fdef.name,
        has_docstring=_has_docstring(fdef),
        source_file=mod.rel_path,
        line=fdef.lineno,
    )


__all__ = ["RouteInfo", "extract_static_routes", "route_info_from_live_route"]
