from __future__ import annotations

"""Correctness-focused static checks."""

import ast

from .. import project
from ..models import DoctorIssue

def check_sync_io_in_async() -> list[DoctorIssue]:
    """Synchronous I/O in async handlers blocks the event loop — use async alternatives.

    FastAPI runs async handlers on the event loop. Calling open(), time.sleep(),
    or synchronous HTTP clients (requests.*) blocks all concurrent requests.
    Use aiofiles, asyncio.sleep(), or httpx.AsyncClient instead.
    """
    issues: list[DoctorIssue] = []
    # Sync I/O patterns that block the event loop
    sync_io_calls = {
        "open": "Use aiofiles.open() or run in a thread with asyncio.to_thread().",
        "sleep": "Use asyncio.sleep() instead of time.sleep().",
    }
    sync_http_attrs = {"get", "post", "put", "patch", "delete", "head", "request"}

    router_dir = project.OWN_CODE_DIR / "routers"
    if not router_dir.is_dir():
        return issues
    for module in project.parsed_python_modules():
        if not module.path.is_relative_to(router_dir):
            continue
        if "async " not in module.source:
            continue
        for node in ast.walk(module.tree):
            if not isinstance(node, ast.AsyncFunctionDef):
                continue
            for child in ast.walk(node):
                if not isinstance(child, ast.Call):
                    continue
                func = child.func
                # Check bare calls: open(), time.sleep()
                if isinstance(func, ast.Name) and func.id in sync_io_calls:
                    # open() is OK if used with Path.read_text() or in non-file contexts
                    issues.append(
                        DoctorIssue(
                            check="correctness/sync-io-in-async",
                            severity="error",
                            message=f"Sync I/O call '{func.id}()' inside async handler '{node.name}' blocks the event loop",
                            path=module.rel_path,
                            category="Correctness",
                            help=sync_io_calls[func.id],
                            line=child.lineno,
                        )
                    )
                # Check attribute calls: time.sleep(), requests.get()
                elif isinstance(func, ast.Attribute):
                    if isinstance(func.value, ast.Name):
                        if func.value.id == "time" and func.attr == "sleep":
                            issues.append(
                                DoctorIssue(
                                    check="correctness/sync-io-in-async",
                                    severity="error",
                                    message=f"time.sleep() inside async handler '{node.name}' blocks the event loop",
                                    path=module.rel_path,
                                    category="Correctness",
                                    help="Use asyncio.sleep() instead.",
                                    line=child.lineno,
                                )
                            )
                        elif func.value.id == "requests" and func.attr in sync_http_attrs:
                            issues.append(
                                DoctorIssue(
                                    check="correctness/sync-io-in-async",
                                    severity="error",
                                    message=f"Sync HTTP call 'requests.{func.attr}()' inside async handler '{node.name}' blocks the event loop",
                                    path=module.rel_path,
                                    category="Correctness",
                                    help="Use httpx.AsyncClient or aiohttp instead of the requests library.",
                                    line=child.lineno,
                                )
                            )
    return issues

def check_naive_datetime() -> list[DoctorIssue]:
    """Detect naive datetime usage (now/utcnow).

    Python 3.12+ deprecates naive datetimes. Use datetime.now(tz=UTC).
    """
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "datetime" not in module.source:
            continue
        for node in ast.walk(module.tree):
            if not isinstance(node, ast.Call):
                continue
            func = node.func
            if not isinstance(func, ast.Attribute):
                continue

            # Match datetime.now() or datetime.utcnow()
            is_datetime = False
            if isinstance(func.value, ast.Name) and func.value.id == "datetime":
                is_datetime = True
            elif isinstance(func.value, ast.Attribute) and func.value.attr == "datetime":
                is_datetime = True

            if is_datetime:
                if func.attr == "utcnow":
                    issues.append(
                        DoctorIssue(
                            check="correctness/naive-datetime",
                            severity="warning",
                            message="datetime.utcnow() is deprecated — use datetime.now(tz=UTC)",
                            path=module.rel_path,
                            category="Correctness",
                            help="from datetime import UTC; datetime.now(tz=UTC)",
                            line=node.lineno,
                        )
                    )
                elif func.attr == "now" and not node.keywords and not node.args:
                    issues.append(
                        DoctorIssue(
                            check="correctness/naive-datetime",
                            severity="warning",
                            message="datetime.now() without timezone — use datetime.now(tz=UTC)",
                            path=module.rel_path,
                            category="Correctness",
                            help="from datetime import UTC; datetime.now(tz=UTC)",
                            line=node.lineno,
                        )
                    )
    return issues


def check_avoid_os_path() -> list[DoctorIssue]:
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "os.path" not in module.source:
            continue
        for node in ast.walk(module.tree):
            if isinstance(node, ast.Attribute):
                if isinstance(node.value, ast.Attribute) and getattr(node.value.value, "id", "") == "os" and node.value.attr == "path":
                    issues.append(DoctorIssue(
                        check="correctness/avoid-os-path",
                        severity="warning",
                        message=f"os.path.{node.attr} usage detected — prefer pathlib.Path",
                        path=module.rel_path,
                        category="Correctness",
                        help="pathlib offers a safer, more robust object-oriented API for paths.",
                        line=node.lineno
                    ))
    return issues


def check_asyncio_run_in_async_context() -> list[DoctorIssue]:
    """Detect asyncio.run() in files that define async functions.

    ``asyncio.run()`` creates a new event loop and blocks until complete.
    Calling it from within an async context (or a file that participates in
    async execution) will either raise RuntimeError('This event loop is
    already running') or cause deadlocks when called from executor threads.
    Use ``await`` directly or ``asyncio.create_task()`` instead.
    """
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "asyncio" not in module.source:
            continue
        if module.path.name in ("__main__.py", "cli.py") or "scripts/" in str(module.path):
            continue

        has_async_def = any(isinstance(n, ast.AsyncFunctionDef) for n in ast.walk(module.tree))
        if not has_async_def:
            continue

        lines = module.source.splitlines()
        for node in ast.walk(module.tree):
            if isinstance(node, ast.Call):
                func = node.func
                if (isinstance(func, ast.Attribute)
                    and isinstance(func.value, ast.Name)
                    and func.value.id == "asyncio"
                    and func.attr == "run"):
                    # Check noqa
                    if node.lineno <= len(lines) and "# noqa" in lines[node.lineno - 1]:
                        continue
                    issues.append(DoctorIssue(
                        check="correctness/asyncio-run-in-async",
                        severity="error",
                        message="asyncio.run() in a module with async functions — use await or create_task instead",
                        path=module.rel_path,
                        category="Correctness",
                        help="asyncio.run() creates a new loop and blocks. In async code, use 'await' directly.",
                        line=node.lineno,
                    ))
    return issues

def check_threading_lock_in_async() -> list[DoctorIssue]:
    """Detect threading.Lock() used in modules with async functions.

    ``threading.Lock()`` is a blocking synchronization primitive. In async code
    it blocks the entire event loop while waiting to acquire. Use
    ``asyncio.Lock()`` instead for cooperative locking, or document why
    a threading lock is intentionally needed (cross-thread sync).
    """
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "Lock" not in module.source:
            continue
        has_async_def = any(isinstance(n, ast.AsyncFunctionDef) for n in ast.walk(module.tree))
        if not has_async_def:
            continue

        lines = module.source.splitlines()
        for node in ast.walk(module.tree):
            if isinstance(node, ast.Call):
                func = node.func
                is_threading_lock = False
                # threading.Lock()
                if (isinstance(func, ast.Attribute)
                    and func.attr == "Lock"
                    and isinstance(func.value, ast.Name)
                    and func.value.id == "threading"):
                    is_threading_lock = True
                # Lock() after 'from threading import Lock'
                elif isinstance(func, ast.Name) and func.id == "Lock":
                    for imp_node in ast.walk(module.tree):
                        if (isinstance(imp_node, ast.ImportFrom)
                            and imp_node.module == "threading"
                            and any(a.name == "Lock" for a in imp_node.names)):
                            is_threading_lock = True
                            break
                if is_threading_lock:
                    if node.lineno <= len(lines) and "# noqa" in lines[node.lineno - 1]:
                        continue
                    issues.append(DoctorIssue(
                        check="correctness/threading-lock-in-async",
                        severity="warning",
                        message="threading.Lock() in async module — blocks event loop; use asyncio.Lock()",
                        path=module.rel_path,
                        category="Correctness",
                        help="threading.Lock blocks the event loop. Use asyncio.Lock for async code, or add '# noqa' if cross-thread sync is intentional.",
                        line=node.lineno,
                    ))
    return issues


def check_deprecated_typing_imports() -> list[DoctorIssue]:
    """Detect deprecated typing imports available as builtins since Python 3.9+.

    ``from typing import List, Dict, Tuple, Set, Optional, Union`` are unnecessary
    in Python 3.9+ — use ``list``, ``dict``, ``tuple``, ``set``, ``X | None``.
    ``from __future__ import annotations`` makes this work in 3.7+.
    """
    _DEPRECATED = frozenset({"List", "Dict", "Tuple", "Set", "FrozenSet", "Type", "Optional", "Union"})
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "from typing import" not in module.source:
            continue
        lines = module.source.splitlines()

        for node in ast.walk(module.tree):
            if isinstance(node, ast.ImportFrom) and node.module == "typing":
                deprecated_names = [a.name for a in node.names if a.name in _DEPRECATED]
                if deprecated_names:
                    if node.lineno <= len(lines) and "# noqa" in lines[node.lineno - 1]:
                        continue
                    issues.append(DoctorIssue(
                        check="correctness/deprecated-typing-imports",
                        severity="warning",
                        message=f"Deprecated typing imports: {', '.join(deprecated_names)} — use builtins",
                        path=module.rel_path,
                        category="Correctness",
                        help="Use list, dict, tuple, set, X | None directly. Add 'from __future__ import annotations' for 3.7+ compat.",
                        line=node.lineno,
                    ))
    return issues


def check_mutable_default_arg() -> list[DoctorIssue]:
    """Detect mutable default arguments in function definitions.

    ``def foo(items=[])`` or ``def foo(data={})`` is a classic Python gotcha:
    the default object is shared across all calls, causing subtle mutation bugs.
    Use ``None`` as default and create the mutable inside the function body.
    """
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        lines = module.source.splitlines()

        for node in ast.walk(module.tree):
            if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                continue
            for default in node.args.defaults + node.args.kw_defaults:
                if default is None:
                    continue
                is_mutable = False
                if isinstance(default, ast.List):
                    is_mutable = True
                elif isinstance(default, ast.Dict):
                    is_mutable = True
                elif isinstance(default, ast.Set):
                    is_mutable = True
                elif isinstance(default, ast.Call):
                    # dict(), list(), set() calls
                    if isinstance(default.func, ast.Name) and default.func.id in ("list", "dict", "set"):
                        is_mutable = True
                if is_mutable:
                    if default.lineno <= len(lines) and "# noqa" in lines[default.lineno - 1]:
                        continue
                    issues.append(DoctorIssue(
                        check="correctness/mutable-default-arg",
                        severity="error",
                        message=f"Mutable default argument in {node.name}() — shared across calls",
                        path=module.rel_path,
                        category="Correctness",
                        help="Use None as default: def foo(items=None): items = items or []",
                        line=default.lineno,
                    ))
    return issues

def check_return_in_finally() -> list[DoctorIssue]:
    """Detect return statements inside finally blocks.

    A ``return`` in ``finally`` silently swallows any exception that was being
    propagated, including unhandled ones. This makes debugging nearly impossible
    because the exception vanishes without a trace.
    """
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "finally" not in module.source:
            continue
        lines = module.source.splitlines()

        for node in ast.walk(module.tree):
            if not isinstance(node, ast.Try):
                continue
            for finally_stmt in node.finalbody:
                for child in ast.walk(finally_stmt):
                    if isinstance(child, ast.Return):
                        if child.lineno <= len(lines) and "# noqa" in lines[child.lineno - 1]:
                            continue
                        issues.append(DoctorIssue(
                            check="correctness/return-in-finally",
                            severity="error",
                            message="return inside finally block — silently swallows exceptions",
                            path=module.rel_path,
                            category="Correctness",
                            help="Move the return outside the finally block. finally should only do cleanup.",
                            line=child.lineno,
                        ))
    return issues

def check_unreachable_code() -> list[DoctorIssue]:
    """Detect unreachable code after return, raise, break, or continue.

    Statements following a terminal statement (return, raise, break, continue)
    within the same block will never execute. This is dead code that misleads
    readers and often indicates a logic error.
    """
    _TERMINAL = (ast.Return, ast.Raise, ast.Break, ast.Continue)
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        lines = module.source.splitlines()

        def _check_block(stmts: list[ast.stmt]) -> None:
            for i, stmt in enumerate(stmts):
                if isinstance(stmt, _TERMINAL) and i < len(stmts) - 1:
                    next_stmt = stmts[i + 1]
                    # Skip if the unreachable code is just a string (docstring/comment)
                    if isinstance(next_stmt, ast.Expr) and isinstance(next_stmt.value, ast.Constant) and isinstance(next_stmt.value.value, str):
                        continue
                    if next_stmt.lineno <= len(lines) and "# noqa" in lines[next_stmt.lineno - 1]:
                        continue
                    issues.append(DoctorIssue(
                        check="correctness/unreachable-code",
                        severity="warning",
                        message=f"Unreachable code after {type(stmt).__name__.lower()} statement",
                        path=module.rel_path,
                        category="Correctness",
                        help="This code never executes. Remove it or fix the control flow logic.",
                        line=next_stmt.lineno,
                    ))
                    break  # Only flag the first unreachable statement per block

        for node in ast.walk(module.tree):
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                _check_block(node.body)
            elif isinstance(node, (ast.If, ast.For, ast.While, ast.With, ast.AsyncWith, ast.AsyncFor)):
                _check_block(node.body)
                if hasattr(node, "orelse") and node.orelse:
                    _check_block(node.orelse)
            elif isinstance(node, ast.ExceptHandler):
                _check_block(node.body)
            elif isinstance(node, ast.Try):
                _check_block(node.body)
                for handler in node.handlers:
                    _check_block(handler.body)
                _check_block(node.orelse)
                _check_block(node.finalbody)
    return issues

def check_get_with_side_effect() -> list[DoctorIssue]:
    """Detect GET endpoints that perform mutations (violates REST semantics).

    Inspired by react-doctor's ``nextjs-no-side-effect-in-get-handler`` rule.
    GET requests must be safe/idempotent. Detects calls to .add(), .delete(),
    .commit(), mutating SQL .execute(), .update(), .remove(), .send(), .post() inside
    functions registered as GET handlers.
    """
    _MUTATION_ATTRS = frozenset({
        "add", "delete", "commit", "update", "remove", "send",
        "post", "put", "patch", "insert", "drop", "create", "save",
        "bulk_save_objects", "merge", "flush",
    })

    def sql_text_is_mutating(call: ast.Call) -> bool:
        if not (
            isinstance(call.func, ast.Attribute)
            and call.func.attr == "execute"
            and call.args
        ):
            return False
        first_arg = call.args[0]
        sql_text = None
        if isinstance(first_arg, ast.Constant) and isinstance(first_arg.value, str):
            sql_text = first_arg.value
        elif isinstance(first_arg, ast.Call) and isinstance(first_arg.func, ast.Name) and first_arg.func.id == "text":
            if first_arg.args and isinstance(first_arg.args[0], ast.Constant) and isinstance(first_arg.args[0].value, str):
                sql_text = first_arg.args[0].value
        if sql_text is None:
            return False
        return sql_text.lstrip().upper().startswith(("INSERT", "UPDATE", "DELETE", "ALTER", "DROP", "CREATE", "TRUNCATE"))
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "routers/" not in module.rel_path and "routes/" not in module.rel_path and "api/" not in module.rel_path:
            continue
        lines = module.source.splitlines()

        # Find functions decorated with @router.get or @app.get
        for node in ast.walk(module.tree):
            if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                continue
            is_get = False
            for decorator in node.decorator_list:
                if isinstance(decorator, ast.Call) and isinstance(decorator.func, ast.Attribute):
                    if decorator.func.attr == "get":
                        is_get = True
                        break
            if not is_get:
                continue

            # Walk the function body for mutation calls
            for child in ast.walk(node):
                if isinstance(child, ast.Call) and isinstance(child.func, ast.Attribute):
                    is_mutation = child.func.attr in _MUTATION_ATTRS or sql_text_is_mutating(child)
                    if is_mutation:
                        if child.lineno <= len(lines) and "# noqa" in lines[child.lineno - 1]:
                            continue
                        issues.append(DoctorIssue(
                            check="correctness/get-with-side-effect",
                            severity="warning",
                            message=f"GET endpoint {node.name}() calls .{child.func.attr}() — violates REST semantics",
                            path=module.rel_path,
                            category="Correctness",
                            help="GET must be safe/idempotent. Move mutations to POST/PUT/DELETE endpoints.",
                            line=child.lineno,
                        ))
                        break  # One flag per function
    return issues


__all__ = [
    "check_asyncio_run_in_async_context",
    "check_avoid_os_path",
    "check_deprecated_typing_imports",
    "check_get_with_side_effect",
    "check_mutable_default_arg",
    "check_naive_datetime",
    "check_return_in_finally",
    "check_sync_io_in_async",
    "check_threading_lock_in_async",
    "check_unreachable_code",
]
