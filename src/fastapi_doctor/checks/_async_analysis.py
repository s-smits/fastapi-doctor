from __future__ import annotations

"""Shared helpers for async/sync static analysis.

Performance: ``build_module_function_index`` is the hot path.  We cache the
index per ``id(module_tree)`` so that multiple checks sharing the same
parsed module avoid redundant walks.  ``_create_function_context`` walks the
function body once and classifies all three properties (async-constructs,
generator, returns-awaitable) in a single pass.
"""

import ast
from dataclasses import dataclass


@dataclass(slots=True)
class FunctionContext:
    """Static metadata for a function or method within one module."""

    node: ast.FunctionDef | ast.AsyncFunctionDef
    name: str
    qualname: str
    owner_class: str | None
    is_async: bool
    is_route_handler: bool
    has_async_constructs: bool
    is_generator: bool
    is_sync_context_manager: bool
    is_async_context_manager: bool
    returns_awaitable: bool


class ModuleFunctionIndex:
    """Index top-level functions and same-class methods for one module."""

    def __init__(self, functions: list[FunctionContext]) -> None:
        self.functions = functions
        self._by_name = {
            function.name: function
            for function in functions
            if function.owner_class is None
        }
        self._by_method = {
            (function.owner_class, function.name): function
            for function in functions
            if function.owner_class is not None
        }

    def get_context(self, qualname: str) -> FunctionContext | None:
        if "." in qualname:
            owner_class, _, name = qualname.rpartition(".")
            return self._by_method.get((owner_class, name))
        return self._by_name.get(qualname)

    def resolve_call(self, caller: FunctionContext, call: ast.Call) -> FunctionContext | None:
        func = call.func
        if isinstance(func, ast.Name):
            return self._by_name.get(func.id)
        if (
            isinstance(func, ast.Attribute)
            and isinstance(func.value, ast.Name)
            and func.value.id in {"self", "cls"}
            and caller.owner_class is not None
        ):
            return self._by_method.get((caller.owner_class, func.attr))
        return None


# ---------------------------------------------------------------------------
# Fast body-node walk — single pass, skips nested defs
# ---------------------------------------------------------------------------

_SKIP_TYPES = (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef, ast.Lambda)
_ASYNC_TYPES = (ast.Await, ast.AsyncFor, ast.AsyncWith, ast.Yield, ast.YieldFrom)
_YIELD_TYPES = (ast.Yield, ast.YieldFrom)
_AWAITABLE_RETURN_NAMES = frozenset({"create_task", "ensure_future", "gather", "shield"})


def function_body_nodes(node: ast.FunctionDef | ast.AsyncFunctionDef) -> list[ast.AST]:
    """Return nodes in a function body, excluding nested defs/classes."""
    nodes: list[ast.AST] = []
    stack = list(reversed(node.body))
    while stack:
        current = stack.pop()
        if isinstance(current, _SKIP_TYPES):
            continue
        nodes.append(current)
        children = list(ast.iter_child_nodes(current))
        if children:
            stack.extend(reversed(children))
    return nodes


def _classify_body(node: ast.FunctionDef | ast.AsyncFunctionDef) -> tuple[bool, bool, bool]:
    """Single pass: (has_async_constructs, is_generator, returns_awaitable)."""
    has_async = False
    is_gen = False
    returns_awaitable = False
    stack = list(reversed(node.body))
    while stack:
        current = stack.pop()
        if isinstance(current, _SKIP_TYPES):
            continue
        if isinstance(current, _ASYNC_TYPES):
            has_async = True
            if isinstance(current, _YIELD_TYPES):
                is_gen = True
        elif isinstance(current, ast.Return) and current.value is not None:
            val = current.value
            if isinstance(val, ast.Call):
                f = val.func
                if isinstance(f, ast.Attribute) and f.attr in _AWAITABLE_RETURN_NAMES:
                    returns_awaitable = True
                elif isinstance(f, ast.Name) and f.id in _AWAITABLE_RETURN_NAMES:
                    returns_awaitable = True
        for child in ast.iter_child_nodes(current):
            stack.append(child)
    return has_async, is_gen, returns_awaitable


def function_has_async_constructs(node: ast.FunctionDef | ast.AsyncFunctionDef) -> bool:
    """True when the function body contains async-only constructs."""
    return _classify_body(node)[0]


def function_is_generator(node: ast.FunctionDef | ast.AsyncFunctionDef) -> bool:
    """True when the function body contains yield or yield from."""
    return _classify_body(node)[1]


# ---------------------------------------------------------------------------
# Cached module index
# ---------------------------------------------------------------------------

_INDEX_CACHE: dict[int, ModuleFunctionIndex] = {}


def build_module_function_index(module_tree: ast.AST) -> ModuleFunctionIndex:
    """Index top-level functions and class methods — cached by tree identity."""
    tree_id = id(module_tree)
    cached = _INDEX_CACHE.get(tree_id)
    if cached is not None:
        return cached
    functions: list[FunctionContext] = []
    for stmt in getattr(module_tree, "body", []):
        if isinstance(stmt, (ast.FunctionDef, ast.AsyncFunctionDef)):
            functions.append(_create_function_context(stmt))
        elif isinstance(stmt, ast.ClassDef):
            for class_stmt in stmt.body:
                if isinstance(class_stmt, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    functions.append(_create_function_context(class_stmt, owner_class=stmt.name))
    index = ModuleFunctionIndex(functions)
    _INDEX_CACHE[tree_id] = index
    return index


def clear_index_cache() -> None:
    _INDEX_CACHE.clear()


def _create_function_context(
    node: ast.FunctionDef | ast.AsyncFunctionDef, owner_class: str | None = None
) -> FunctionContext:
    is_async = isinstance(node, ast.AsyncFunctionDef)
    name = node.name
    qualname = f"{owner_class}.{name}" if owner_class else name

    # Single pass over the body for all three classifications
    has_async, is_gen, returns_awaitable = _classify_body(node)

    return FunctionContext(
        node=node,
        name=name,
        qualname=qualname,
        owner_class=owner_class,
        is_async=is_async,
        is_route_handler=_looks_like_route_handler(node),
        has_async_constructs=has_async,
        is_generator=is_gen,
        is_sync_context_manager=_looks_like_context_manager(node, async_only=False),
        is_async_context_manager=_looks_like_context_manager(node, async_only=True),
        returns_awaitable=returns_awaitable,
    )


def _looks_like_route_handler(node: ast.FunctionDef | ast.AsyncFunctionDef) -> bool:
    for decorator in node.decorator_list:
        dump = ast.dump(decorator).lower()
        if "router" in dump or "app" in dump:
            return True
    return False


def _looks_like_context_manager(node: ast.FunctionDef | ast.AsyncFunctionDef, async_only: bool = False) -> bool:
    for decorator in node.decorator_list:
        dec_name = ""
        if isinstance(decorator, ast.Name):
            dec_name = decorator.id
        elif isinstance(decorator, ast.Attribute):
            dec_name = decorator.attr
        elif isinstance(decorator, ast.Call):
            if isinstance(decorator.func, ast.Name):
                dec_name = decorator.func.id
            elif isinstance(decorator.func, ast.Attribute):
                dec_name = decorator.func.attr
        dec_name = dec_name.lower()
        if async_only:
            if "asynccontextmanager" in dec_name:
                return True
        else:
            if "contextmanager" in dec_name and "async" not in dec_name:
                return True
    return False


__all__ = [
    "FunctionContext",
    "ModuleFunctionIndex",
    "build_module_function_index",
    "clear_index_cache",
    "function_body_nodes",
    "function_has_async_constructs",
]
