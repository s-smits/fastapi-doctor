from __future__ import annotations

"""Architecture-focused static checks."""

import ast

from .. import project
from ..models import DoctorIssue

def check_giant_functions() -> list[DoctorIssue]:
    """Functions exceeding size thresholds are hard to test and maintain."""
    if project.GIANT_FUNCTION_THRESHOLD <= 0 and project.LARGE_FUNCTION_THRESHOLD <= 0:
        return []
    issues: list[DoctorIssue] = []
    for filepath in project.own_python_files():
        try:
            source = filepath.read_text()
            tree = ast.parse(source)
        except Exception:
            continue
        for node in ast.walk(tree):
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                # Allow # noqa: architecture to suppress architectural warnings
                source_segment = ast.get_source_segment(source, node)
                if source_segment and "# noqa: architecture" in source_segment:
                    continue

                size = (node.end_lineno or node.lineno) - node.lineno + 1
                rel_path = str(filepath.relative_to(project.REPO_ROOT))
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
    """async def that never awaits wastes an event loop slot — use plain def."""
    issues: list[DoctorIssue] = []
    router_dir = project.OWN_CODE_DIR / "routers"
    if not router_dir.is_dir():
        return issues
    for filepath in sorted(router_dir.rglob("*.py")):
        if "__pycache__" in str(filepath):
            continue
        try:
            source = filepath.read_text()
            tree = ast.parse(source)
        except Exception:
            continue
        for node in ast.walk(tree):
            if not isinstance(node, ast.AsyncFunctionDef):
                continue
            if node.name in project.ASYNC_ENDPOINT_NOAWAIT_EXCLUDE:
                continue
            # Check for any async operation
            has_async_op = any(
                isinstance(n, (ast.Await, ast.AsyncFor, ast.AsyncWith, ast.YieldFrom, ast.Yield))
                for n in ast.walk(node)
            )
            if not has_async_op:
                # Only flag if it looks like a route handler (has decorators)
                is_route = any(
                    "router" in ast.dump(d).lower() or "app" in ast.dump(d).lower() for d in node.decorator_list
                )
                if is_route:
                    issues.append(
                        DoctorIssue(
                            check="architecture/async-without-await",
                            severity="warning",
                            message=f"async def '{node.name}' never awaits — use plain def to avoid blocking the event loop",
                            path=str(filepath.relative_to(project.REPO_ROOT)),
                            category="Architecture",
                            help="FastAPI runs plain def endpoints in a thread pool, which is safer for sync code.",
                            line=node.lineno,
                        )
                    )
    return issues

def check_print_statements() -> list[DoctorIssue]:
    """Production code should use the logger, not print()."""
    issues: list[DoctorIssue] = []
    exclude_dirs = {"scripts", "lib"}
    for filepath in project.own_python_files():
        parts = filepath.relative_to(project.OWN_CODE_DIR).parts
        if parts and parts[0] in exclude_dirs:
            continue
        try:
            source = filepath.read_text()
            tree = ast.parse(source)
        except Exception:
            continue
        for node in ast.walk(tree):
            if isinstance(node, ast.Call):
                func = node.func
                if isinstance(func, ast.Name) and func.id == "print":
                    issues.append(
                        DoctorIssue(
                            check="architecture/print-in-production",
                            severity="warning",
                            message="print() in production code — use logger instead",
                            path=str(filepath.relative_to(project.REPO_ROOT)),
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
    for filepath in project.own_python_files():
        try:
            source = filepath.read_text()
            if "# noqa: architecture" in source:
                continue
            line_count = len(source.splitlines())
        except Exception:
            continue
        if line_count > project.GOD_MODULE_THRESHOLD:
            issues.append(
                DoctorIssue(
                    check="architecture/god-module",
                    severity="warning",
                    message=f"File is {line_count} lines (>{project.GOD_MODULE_THRESHOLD}) — decompose into focused modules",
                    path=str(filepath.relative_to(project.REPO_ROOT)),
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
    for filepath in project.own_python_files():
        try:
            source = filepath.read_text()
            tree = ast.parse(source)
        except Exception:
            continue
        for node in ast.walk(tree):
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                # Allow # noqa: architecture to suppress architectural warnings
                source_segment = ast.get_source_segment(source, node)
                if source_segment and "# noqa: architecture" in source_segment:
                    continue

                depth = _max_nesting_depth(node)

                if depth > project.DEEP_NESTING_THRESHOLD:
                    issues.append(
                        DoctorIssue(
                            check="architecture/deep-nesting",
                            severity="warning",
                            message=f"Function '{node.name}' has {depth} levels of nesting (>{project.DEEP_NESTING_THRESHOLD}) — extract inner logic",
                            path=str(filepath.relative_to(project.REPO_ROOT)),
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
    for filepath in project.own_python_files():
        if filepath.name in exempt_names:
            continue
        try:
            source = filepath.read_text()
            if "# noqa: architecture" in source:
                continue
            tree = ast.parse(source)
        except Exception:
            continue
        import_count = sum(1 for node in ast.walk(tree) if isinstance(node, (ast.Import, ast.ImportFrom)))
        if import_count > threshold:
            issues.append(
                DoctorIssue(
                    check="architecture/import-bloat",
                    severity="warning",
                    message=f"File has {import_count} imports (>{threshold}) — consider decomposing",
                    path=str(filepath.relative_to(project.REPO_ROOT)),
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
    for filepath in project.own_python_files():
        try:
            source = filepath.read_text()
            tree = ast.parse(source)
        except Exception:
            continue
        for node in ast.walk(tree):
            if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                continue
            # Skip methods (inside classes)
            if any(
                isinstance(parent, ast.ClassDef) for parent in ast.walk(tree) if node in ast.iter_child_nodes(parent)
            ):
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
                        path=str(filepath.relative_to(project.REPO_ROOT)),
                        category="Architecture",
                        help="This function just delegates to another. Inline it or add a docstring explaining why the wrapper exists.",
                        line=node.lineno,
                    )
                )
    return issues


def check_avoid_sys_exit() -> list[DoctorIssue]:
    issues: list[DoctorIssue] = []
    for filepath in project.own_python_files():
        if filepath.name in ("__main__.py", "cli.py") or "scripts/" in str(filepath):
            continue
        try:
            tree = ast.parse(filepath.read_text())
        except Exception:
            continue
            
        for node in ast.walk(tree):
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
                        path=str(filepath.relative_to(project.REPO_ROOT)),
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
    for filepath in project.own_python_files():
        if filepath.name == "__init__.py":
            continue  # __init__.py re-exports are a common valid pattern
        try:
            source = filepath.read_text()
            tree = ast.parse(source)
        except Exception:
            continue
        rel_path = str(filepath.relative_to(project.REPO_ROOT))
        lines = source.splitlines()

        for node in ast.walk(tree):
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

__all__ = [
    "_max_nesting_depth",
    "check_async_without_await",
    "check_avoid_sys_exit",
    "check_deep_nesting",
    "check_giant_functions",
    "check_god_modules",
    "check_import_bloat",
    "check_passthrough_functions",
    "check_print_statements",
    "check_star_import",
]
