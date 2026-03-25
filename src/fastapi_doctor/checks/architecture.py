from __future__ import annotations

"""Architecture-focused static checks."""

import ast
import re

from .. import project
from ..models import DoctorIssue
from ._async_analysis import build_module_function_index, function_body_nodes, function_has_async_constructs

def check_giant_functions() -> list[DoctorIssue]:
    """Functions exceeding size thresholds are hard to test and maintain."""
    if project.GIANT_FUNCTION_THRESHOLD <= 0 and project.LARGE_FUNCTION_THRESHOLD <= 0:
        return []
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        for node in ast.walk(module.tree):
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                # Allow # noqa: architecture to suppress architectural warnings
                source_segment = ast.get_source_segment(module.source, node)
                if source_segment and "# noqa: architecture" in source_segment:
                    continue

                size = (node.end_lineno or node.lineno) - node.lineno + 1
                rel_path = module.rel_path
                if project.GIANT_FUNCTION_THRESHOLD > 0 and size > project.GIANT_FUNCTION_THRESHOLD:
                    issues.append(
                        DoctorIssue(
                            check="architecture/giant-function",
                            severity="error",
                            message=f"Function '{node.name}' is {size} lines (>{project.GIANT_FUNCTION_THRESHOLD}) — extract sub-functions",
                            path=rel_path,
                            category="Architecture",
                            help="Break into smaller, testable functions. Each should do one thing.",
                            line=node.lineno,
                        )
                    )
                elif project.LARGE_FUNCTION_THRESHOLD > 0 and size > project.LARGE_FUNCTION_THRESHOLD:
                    issues.append(
                        DoctorIssue(
                            check="architecture/large-function",
                            severity="warning",
                            message=f"Function '{node.name}' is {size} lines (>{project.LARGE_FUNCTION_THRESHOLD}) — consider splitting",
                            path=rel_path,
                            category="Architecture",
                            help="Functions over 200 lines are harder to maintain and test.",
                            line=node.lineno,
                        )
                    )
    return issues

def check_async_without_await() -> list[DoctorIssue]:
    """async def that never awaits wastes an event loop slot — use plain def.

    This check is 'transitive': it flags functions that have no async constructs
    of their own, AND functions that only await other functions already flagged
    as unnecessary async.
    """
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        function_index = build_module_function_index(module.tree)
        unnecessary_async: set[str] = set()

        # Phase 1: Direct detection (no async constructs)
        for function_ctx in function_index.functions:
            if not function_ctx.is_async:
                continue
            if function_ctx.name in project.ASYNC_ENDPOINT_NOAWAIT_EXCLUDE:
                continue
            if not function_ctx.has_async_constructs:
                unnecessary_async.add(function_ctx.qualname)

        # Phase 2: Transitive detection
        # If an async function ONLY awaits functions in unnecessary_async, it's also unnecessary.
        changed = True
        while changed:
            changed = False
            for function_ctx in function_index.functions:
                if not function_ctx.is_async or function_ctx.qualname in unnecessary_async:
                    continue
                if function_ctx.name in project.ASYNC_ENDPOINT_NOAWAIT_EXCLUDE:
                    continue

                # Must only have Await nodes, and all must resolve to unnecessary functions
                body_nodes = function_body_nodes(function_ctx.node)
                has_other_async = any(
                    isinstance(n, (ast.AsyncFor, ast.AsyncWith)) for n in body_nodes
                )
                if has_other_async:
                    continue

                awaits = [n for n in body_nodes if isinstance(n, ast.Await)]
                if not awaits:
                    continue  # Should have been caught in Phase 1 if it had no awaits at all

                all_awaits_unnecessary = True
                for await_node in awaits:
                    if not isinstance(await_node.value, ast.Call):
                        all_awaits_unnecessary = False
                        break
                    resolved = function_index.resolve_call(function_ctx, await_node.value)
                    if not resolved or resolved.qualname not in unnecessary_async:
                        all_awaits_unnecessary = False
                        break

                if all_awaits_unnecessary:
                    unnecessary_async.add(function_ctx.qualname)
                    changed = True

        for function_ctx in function_index.functions:
            if function_ctx.qualname not in unnecessary_async:
                continue

            if function_ctx.is_route_handler:
                message = (
                    f"Async route handler '{function_ctx.qualname}' is effectively synchronous — "
                    "use plain def to avoid blocking the event loop"
                )
                help_text = (
                    "FastAPI runs plain def endpoints in a thread pool. This handler either has "
                    "no awaits or only awaits other functions that don't do real async work."
                )
            else:
                message = (
                    f"async def '{function_ctx.qualname}' is effectively synchronous — convert "
                    "to plain def unless it must maintain an awaitable interface"
                )
                help_text = (
                    "This function contains no real async work (awaits, async for/with). "
                    "Reserve async def for truly awaitable operations."
                )

            issues.append(
                DoctorIssue(
                    check="architecture/async-without-await",
                    severity="warning",
                    message=message,
                    path=module.rel_path,
                    category="Architecture",
                    help=help_text,
                    line=function_ctx.node.lineno,
                )
            )
    return issues

def check_print_statements() -> list[DoctorIssue]:
    """Production code should use the logger, not print()."""
    issues: list[DoctorIssue] = []
    exclude_dirs = {"scripts", "lib"}
    for module in project.parsed_python_modules():
        if "print(" not in module.source:
            continue
        parts = module.path.relative_to(project.OWN_CODE_DIR).parts
        if parts and parts[0] in exclude_dirs:
            continue
        for node in ast.walk(module.tree):
            if isinstance(node, ast.Call):
                func = node.func
                if isinstance(func, ast.Name) and func.id == "print":
                    issues.append(
                        DoctorIssue(
                            check="architecture/print-in-production",
                            severity="warning",
                            message="print() in production code — use logger instead",
                            path=module.rel_path,
                            category="Architecture",
                            help="Replace with logger.info/debug/warning as appropriate.",
                            line=node.lineno,
                        )
                    )
    return issues

def check_god_modules() -> list[DoctorIssue]:
    """Files over 1500 lines need decomposition — they're untestable monoliths."""
    if project.GOD_MODULE_THRESHOLD <= 0:
        return []
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        source = module.source
        if "# noqa: architecture" in source:
            continue
        line_count = len(source.splitlines())
        if line_count > project.GOD_MODULE_THRESHOLD:
            issues.append(
                DoctorIssue(
                    check="architecture/god-module",
                    severity="warning",
                    message=f"File is {line_count} lines (>{project.GOD_MODULE_THRESHOLD}) — decompose into focused modules",
                    path=module.rel_path,
                    category="Architecture",
                    help="Extract cohesive groups of functions into separate modules. Each module should have one reason to change.",
                )
            )
    return issues

def _max_nesting_depth(node: ast.AST) -> int:
    """Compute the maximum control-flow nesting depth within an AST node."""
    nesting_types = (ast.If, ast.For, ast.While, ast.With, ast.AsyncFor, ast.AsyncWith, ast.Try)

    def _walk_depth(n: ast.AST, depth: int) -> int:
        max_d = depth
        for child in ast.iter_child_nodes(n):
            if isinstance(child, nesting_types):
                max_d = max(max_d, _walk_depth(child, depth + 1))
            elif isinstance(child, ast.ExceptHandler):
                # except blocks are siblings of try, same nesting level
                max_d = max(max_d, _walk_depth(child, depth))
            else:
                max_d = max(max_d, _walk_depth(child, depth))
        return max_d

    return _walk_depth(node, 0)

def check_deep_nesting() -> list[DoctorIssue]:
    """Functions with >5 levels of control-flow nesting are unreadable.

    Deep nesting (if → for → try → if → with → ...) makes code hard to follow,
    test, and modify. Extract inner blocks into named helper functions.
    """
    if project.DEEP_NESTING_THRESHOLD <= 0:
        return []
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        for node in ast.walk(module.tree):
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                # Allow # noqa: architecture to suppress architectural warnings
                source_segment = ast.get_source_segment(module.source, node)
                if source_segment and "# noqa: architecture" in source_segment:
                    continue

                depth = _max_nesting_depth(node)

                if depth > project.DEEP_NESTING_THRESHOLD:
                    issues.append(
                        DoctorIssue(
                            check="architecture/deep-nesting",
                            severity="warning",
                            message=f"Function '{node.name}' has {depth} levels of nesting (>{project.DEEP_NESTING_THRESHOLD}) — extract inner logic",
                            path=module.rel_path,
                            category="Architecture",
                            help="Use early returns, guard clauses, or extract nested blocks into helper functions.",
                            line=node.lineno,
                        )
                    )
    return issues


def check_import_bloat() -> list[DoctorIssue]:
    """Files with >30 imports signal poor module decomposition.

    AST-based: counts Import and ImportFrom nodes. High import count means
    the module depends on too many things — it should be split into focused
    sub-modules. __init__.py and main.py are exempt (they're aggregators).
    """
    threshold = project._IMPORT_BLOAT_THRESHOLD  # via .fastapi-doctor.yml
    if threshold <= 0:
        return []
    exempt_names = {"__init__.py", "main.py"}
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if module.path.name in exempt_names:
            continue
        if "# noqa: architecture" in module.source:
            continue
        import_count = sum(1 for node in ast.walk(module.tree) if isinstance(node, (ast.Import, ast.ImportFrom)))
        if import_count > threshold:
            issues.append(
                DoctorIssue(
                    check="architecture/import-bloat",
                    severity="warning",
                    message=f"File has {import_count} imports (>{threshold}) — consider decomposing",
                    path=module.rel_path,
                    category="Architecture",
                    help="Use TYPE_CHECKING guards for type-only imports, lazy-import heavy libraries, or split the module.",
                )
            )
    return issues

def check_passthrough_functions() -> list[DoctorIssue]:
    """Functions that purely delegate to another function are unnecessary abstraction.

    AST-based detection: finds functions whose body is a single ``return f(...)``
    where the arguments to ``f`` are exactly the function's own parameters (possibly
    reordered). These add indirection without value — inline them or document why
    the wrapper exists.

    Smart exemptions:
    - Pydantic validators (@field_validator, @validator, @model_validator)
    - Property getters (@property)
    - Functions with decorators (likely adding behavior)
    - Methods (self/cls make delegation natural)
    - Functions with docstrings (documented intent)
    - Functions < 2 params (too simple to flag)
    """
    issues: list[DoctorIssue] = []
    decorator_exempt = {
        "property",
        "staticmethod",
        "classmethod",
        "field_validator",
        "validator",
        "model_validator",
        "override",
        "abstractmethod",
        "lru_cache",
        "cache",
        "cached_property",
    }
    for module in project.parsed_python_modules():
        for node in ast.walk(module.tree):
            if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                continue
            # Skip methods (inside classes) by only considering module-level functions.
            if not any(node is top_level for top_level in module.tree.body if isinstance(top_level, (ast.FunctionDef, ast.AsyncFunctionDef))):
                continue
            # Skip decorated functions
            if node.decorator_list:
                has_exempt = False
                for dec in node.decorator_list:
                    dec_name = ""
                    if isinstance(dec, ast.Name):
                        dec_name = dec.id
                    elif isinstance(dec, ast.Attribute):
                        dec_name = dec.attr
                    elif isinstance(dec, ast.Call):
                        if isinstance(dec.func, ast.Name):
                            dec_name = dec.func.id
                        elif isinstance(dec.func, ast.Attribute):
                            dec_name = dec.func.attr
                    if dec_name in decorator_exempt or "router" in dec_name.lower():
                        has_exempt = True
                        break
                if has_exempt or node.decorator_list:
                    continue
            # Skip functions with docstrings
            if (
                node.body
                and isinstance(node.body[0], ast.Expr)
                and isinstance(node.body[0].value, (ast.Constant, ast.Str))
            ):
                continue
            # Must have exactly 1 statement: return call(...)
            body = node.body
            if len(body) != 1:
                continue
            stmt = body[0]
            if not isinstance(stmt, ast.Return) or stmt.value is None:
                continue
            if not isinstance(stmt.value, ast.Call):
                continue
            # Get function's parameter names
            param_names = {arg.arg for arg in node.args.args}
            if len(param_names) < 2:
                continue  # Too simple to flag
            # Get call's argument names
            call = stmt.value
            call_arg_names: set[str] = set()
            for arg in call.args:
                if isinstance(arg, ast.Name):
                    call_arg_names.add(arg.id)
                elif isinstance(arg, ast.Starred) and isinstance(arg.value, ast.Name):
                    call_arg_names.add(arg.value.id)
            for kw in call.keywords:
                if isinstance(kw.value, ast.Name):
                    call_arg_names.add(kw.value.id)
            # Check if the call uses (a subset of) the same parameters
            if param_names.issubset(call_arg_names) or call_arg_names == param_names:
                issues.append(
                    DoctorIssue(
                        check="architecture/passthrough-function",
                        severity="warning",
                        message=f"Function '{node.name}' is a pure passthrough — consider inlining",
                        path=module.rel_path,
                        category="Architecture",
                        help="This function just delegates to another. Inline it or add a docstring explaining why the wrapper exists.",
                        line=node.lineno,
                    )
                )
    return issues


def check_avoid_sys_exit() -> list[DoctorIssue]:
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "sys.exit" not in module.source and "exit(" not in module.source and "quit(" not in module.source:
            continue
        if module.path.name in ("__main__.py", "cli.py") or "scripts/" in str(module.path):
            continue
        for node in ast.walk(module.tree):
            if isinstance(node, ast.Call):
                is_exit = False
                if isinstance(node.func, ast.Attribute) and getattr(node.func.value, "id", "") == "sys" and node.func.attr == "exit":
                    is_exit = True
                elif isinstance(node.func, ast.Name) and node.func.id in ("exit", "quit"):
                    is_exit = True
                    
                if is_exit:
                    issues.append(DoctorIssue(
                        check="architecture/avoid-sys-exit",
                        severity="warning",
                        message="sys.exit() or quit() in library code — raise an Exception instead",
                        path=module.rel_path,
                        category="Architecture",
                        help="Deep application logic should raise exceptions, not abruptly kill the process.",
                        line=node.lineno
                    ))
    return issues


def check_star_import() -> list[DoctorIssue]:
    """Detect ``from module import *`` statements.

    Inspired by react-doctor's ``no-barrel-import`` rule. Star imports pollute
    the namespace, make it impossible to determine where names come from, break
    static analysis and IDE support, and can silently shadow existing names.
    """
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "import *" not in module.source:
            continue
        if module.path.name == "__init__.py":
            continue  # __init__.py re-exports are a common valid pattern
        rel_path = module.rel_path
        lines = module.source.splitlines()

        for node in ast.walk(module.tree):
            if isinstance(node, ast.ImportFrom):
                if any(alias.name == "*" for alias in node.names):
                    if node.lineno <= len(lines) and "# noqa" in lines[node.lineno - 1]:
                        continue
                    issues.append(DoctorIssue(
                        check="architecture/star-import",
                        severity="warning",
                        message=f"from {node.module} import * — pollutes namespace and breaks static analysis",
                        path=rel_path,
                        category="Architecture",
                        help="Import specific names: from module import Name1, Name2",
                        line=node.lineno,
                    ))
    return issues

def check_engine_pool_pre_ping() -> list[DoctorIssue]:
    """SQLAlchemy engines should use pool_pre_ping=True for robustness.

    Connection poolers (like Supavisor or PgBouncer) and cloud environments often
    drop idle connections. Enabling pool_pre_ping ensures the engine checks the
    connection health before use, avoiding 'server closed the connection' errors.
    """
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        for node in ast.walk(module.tree):
            if isinstance(node, ast.Call):
                func_name = ""
                if isinstance(node.func, ast.Name):
                    func_name = node.func.id
                elif isinstance(node.func, ast.Attribute):
                    func_name = node.func.attr

                if func_name in ("create_engine", "create_async_engine"):
                    has_pre_ping = any(
                        kw.arg == "pool_pre_ping"
                        and isinstance(kw.value, ast.Constant)
                        and kw.value.value is True
                        for kw in node.keywords
                    )
                    if not has_pre_ping:
                        issues.append(
                            DoctorIssue(
                                check="architecture/engine-pool-pre-ping",
                                severity="warning",
                                message=f"{func_name}() called without pool_pre_ping=True",
                                path=module.rel_path,
                                category="Architecture",
                                help="Set pool_pre_ping=True to automatically recover from dropped connections.",
                                line=node.lineno,
                            )
                        )
    return issues


def check_startup_validation() -> list[DoctorIssue]:
    """Detect presence of startup configuration validation.

    Production apps should 'fail fast' during startup if critical configurations
    (like database URL, secrets, or CORS) are missing or invalid.
    """
    issues: list[DoctorIssue] = []
    validation_patterns = re.compile(
        r"(?:validate_.*_startup|settings\.validate|check_config|verify_env)",
        re.IGNORECASE,
    )

    for module in project.parsed_python_modules():
        if module.path.name != "main.py":
            continue

        has_validation = bool(validation_patterns.search(module.source))
        if not has_validation:
            issues.append(
                DoctorIssue(
                    check="architecture/missing-startup-validation",
                    severity="warning",
                    message="Main app entry point missing explicit startup configuration validation",
                    path=module.rel_path,
                    category="Architecture",
                    help="Add a 'fail-fast' validation step during app startup to verify critical settings.",
                    line=1,
                )
            )
            break  # Only flag once for the project
    return issues


__all__ = [
    "_max_nesting_depth",
    "check_async_without_await",
    "check_avoid_sys_exit",
    "check_deep_nesting",
    "check_engine_pool_pre_ping",
    "check_giant_functions",
    "check_god_modules",
    "check_import_bloat",
    "check_passthrough_functions",
    "check_print_statements",
    "check_star_import",
    "check_startup_validation",
]
