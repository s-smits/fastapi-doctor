from __future__ import annotations

"""Shared helpers for async/sync static analysis."""

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
        """Look up a function context by fully qualified name."""
        if "." in qualname:
            owner_class, _, name = qualname.rpartition(".")
            return self._by_method.get((owner_class, name))
        return self._by_name.get(qualname)

    def resolve_call(self, caller: FunctionContext, call: ast.Call) -> FunctionContext | None:
        """Resolve only obvious intra-module calls."""
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


def function_body_nodes(node: ast.FunctionDef | ast.AsyncFunctionDef) -> list[ast.AST]:
    """Return nodes in a function body, excluding nested defs/classes."""
    nodes: list[ast.AST] = []

    def _visit(current: ast.AST) -> None:
        if isinstance(current, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef, ast.Lambda)):
            return
        nodes.append(current)
        for child in ast.iter_child_nodes(current):
            _visit(child)

    for stmt in node.body:
        _visit(stmt)
    return nodes


def function_has_async_constructs(node: ast.FunctionDef | ast.AsyncFunctionDef) -> bool:
    """True when the function body contains async-only constructs.
    
    Yield and YieldFrom in an async function make it an async generator, 
    which is an async construct.
    """
    return any(
        isinstance(
            child,
            (ast.Await, ast.AsyncFor, ast.AsyncWith, ast.Yield, ast.YieldFrom),
        )
        for child in function_body_nodes(node)
    )


def function_is_generator(node: ast.FunctionDef | ast.AsyncFunctionDef) -> bool:
    """True when the function body contains yield or yield from."""
    return any(
        isinstance(child, (ast.Yield, ast.YieldFrom))
        for child in function_body_nodes(node)
    )


def build_module_function_index(module_tree: ast.AST) -> ModuleFunctionIndex:
    """Index top-level functions and top-level class methods for one module."""
    functions: list[FunctionContext] = []
    for stmt in getattr(module_tree, "body", []):
        if isinstance(stmt, (ast.FunctionDef, ast.AsyncFunctionDef)):
            functions.append(_create_function_context(stmt))
        elif isinstance(stmt, ast.ClassDef):
            for class_stmt in stmt.body:
                if isinstance(class_stmt, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    functions.append(_create_function_context(class_stmt, owner_class=stmt.name))
    return ModuleFunctionIndex(functions)


def _create_function_context(
    node: ast.FunctionDef | ast.AsyncFunctionDef, owner_class: str | None = None
) -> FunctionContext:
    is_async = isinstance(node, ast.AsyncFunctionDef)
    name = node.name
    qualname = f"{owner_class}.{name}" if owner_class else name

    has_async = function_has_async_constructs(node)
    is_gen = function_is_generator(node)

    # In async functions, yield makes it an async generator.
    # In sync functions, yield makes it a generator.

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
    )


def _looks_like_route_handler(node: ast.FunctionDef | ast.AsyncFunctionDef) -> bool:
    for decorator in node.decorator_list:
        dump = ast.dump(decorator).lower()
        if "router" in dump or "app" in dump:
            return True
    return False


def _looks_like_context_manager(node: ast.FunctionDef | ast.AsyncFunctionDef, async_only: bool = False) -> bool:
    """Check if function is likely a context manager (via decorator)."""
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
    "function_body_nodes",
    "function_has_async_constructs",
]
