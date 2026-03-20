from __future__ import annotations

"""Performance-oriented static checks."""

import ast

from .. import project
from ..models import DoctorIssue

def check_sequential_awaits() -> list[DoctorIssue]:
    """Detect sequential await expressions that could be parallelised with asyncio.gather().

    Inspired by react-doctor's ``async-parallel`` rule. When two or more ``await``
    calls appear in sequence and operate on independent expressions (no data flow
    from one to the next), they can be run concurrently via ``asyncio.gather()``.

    Heuristic: flags ≥2 consecutive awaited assignments where the awaited calls look
    like value-producing work (not transaction commits, logging, or cleanup) and the
    later await does not depend on names produced by earlier awaits.
    """
    side_effect_only_attrs = {
        "commit",
        "rollback",
        "flush",
        "close",
        "aclose",
        "emit",
        "publish",
        "send",
        "save",
        "delete",
        "create",
        "update",
        "insert",
    }

    def await_call(stmt: ast.stmt) -> ast.Call | None:
        if isinstance(stmt, ast.Assign) and isinstance(stmt.value, ast.Await) and isinstance(stmt.value.value, ast.Call):
            return stmt.value.value
        if isinstance(stmt, ast.AnnAssign) and isinstance(stmt.value, ast.Await) and isinstance(stmt.value.value, ast.Call):
            return stmt.value.value
        return None

    def assigned_names(stmt: ast.stmt) -> set[str]:
        names: set[str] = set()
        targets: list[ast.expr] = []
        if isinstance(stmt, ast.Assign):
            targets = list(stmt.targets)
        elif isinstance(stmt, ast.AnnAssign):
            targets = [stmt.target]
        for target in targets:
            for child in ast.walk(target):
                if isinstance(child, ast.Name):
                    names.add(child.id)
        return names

    # Variable names that indicate a stateful DB session/connection.
    # SQLAlchemy's AsyncSession (and most DB drivers) cannot run concurrent
    # queries on the same session — asyncio.gather() would cause race conditions.
    # See: https://docs.sqlalchemy.org/en/20/orm/extensions/asyncio.html
    _session_hints = frozenset({
        "session", "db", "database", "conn", "connection", "cursor",
        "async_session", "db_session",
    })

    def looks_parallelisable(call: ast.Call) -> bool:
        func = call.func
        if isinstance(func, ast.Attribute):
            if func.attr in side_effect_only_attrs:
                return False
            if func.attr.startswith(("emit_", "log_", "save_", "delete_", "update_", "commit_", "publish_")):
                return False
        elif isinstance(func, ast.Name):
            if func.id.startswith(("emit_", "log_", "save_", "delete_", "update_", "commit_", "publish_")):
                return False
        return True

    def _shared_session_arg(call: ast.Call) -> str | None:
        """Return the session-like variable name if the call is bound to one.

        Detects two patterns:
        - Method call: ``session.execute(...)`` → returns "session"
        - Function call with session first arg: ``helper(session, ...)`` → returns "session"
        """
        func = call.func
        # Method call on a session object: session.execute(...)
        if isinstance(func, ast.Attribute) and isinstance(func.value, ast.Name):
            if func.value.id.lower() in _session_hints:
                return func.value.id
        # Function call with session as first positional arg: helper(session, ...)
        if call.args:
            first = call.args[0]
            if isinstance(first, ast.Name) and first.id.lower() in _session_hints:
                return first.id
        return None

    def _all_share_session(run_calls: list[ast.Call]) -> bool:
        """True when every call in the run operates on the same session variable."""
        names = [_shared_session_arg(c) for c in run_calls]
        return bool(names) and all(n == names[0] and n is not None for n in names)

    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "await " not in module.source:
            continue
        lines = module.source.splitlines()

        for node in ast.walk(module.tree):
            if not isinstance(node, ast.AsyncFunctionDef):
                continue
            body = node.body
            i = 0
            while i < len(body) - 1:
                # Look for runs of awaited assignments that produce values.
                run_start = i
                run: list[ast.stmt] = []
                assigned_so_far: set[str] = set()
                while i < len(body):
                    stmt = body[i]
                    await_expr = await_call(stmt)
                    if await_expr is None or not looks_parallelisable(await_expr):
                        break

                    # Check if this await references any name assigned by a previous await in this run
                    used_names: set[str] = set()
                    for child in ast.walk(await_expr):
                        if isinstance(child, ast.Name):
                            used_names.add(child.id)
                    if used_names & assigned_so_far:
                        # Data dependency — break the run
                        break

                    run.append(stmt)
                    assigned_so_far.update(assigned_names(stmt))
                    i += 1

                if len(run) >= 2:
                    lineno = run[0].lineno
                    if lineno <= len(lines) and "# noqa" in lines[lineno - 1]:
                        i = run_start + len(run)
                        continue
                    # Skip when all calls share the same DB session — they
                    # can't be gathered (AsyncSession is not concurrency-safe).
                    run_calls = [await_call(s) for s in run]
                    if all(c is not None for c in run_calls) and _all_share_session(run_calls):  # type: ignore[arg-type]
                        i = run_start + len(run)
                        continue
                    issues.append(DoctorIssue(
                        check="performance/sequential-awaits",
                        severity="warning",
                        message=f"{len(run)} sequential awaits in {node.name}() could use asyncio.gather()",
                        path=module.rel_path,
                        category="Performance",
                        help="Independent awaits can run concurrently: results = await asyncio.gather(coro1(), coro2())",
                        line=lineno,
                    ))
                i = max(i, run_start + 1)
    return issues

def check_regex_in_loop() -> list[DoctorIssue]:
    """Detect re.compile/match/search/findall with literal patterns inside loops.

    Inspired by react-doctor's ``js-hoist-regexp`` rule. Compiling or matching
    with a string-literal regex inside a for/while loop recompiles on every
    iteration. Hoist the pattern to module level or above the loop.
    """
    _RE_FUNCS = frozenset({"compile", "match", "search", "findall", "fullmatch", "sub", "split"})
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "re." not in module.source:
            continue
        lines = module.source.splitlines()

        # Walk AST, tracking loop depth
        class _LoopVisitor(ast.NodeVisitor):
            def __init__(self) -> None:
                self.loop_depth = 0

            def visit_For(self, node: ast.For) -> None:
                self.loop_depth += 1
                self.generic_visit(node)
                self.loop_depth -= 1

            def visit_While(self, node: ast.While) -> None:
                self.loop_depth += 1
                self.generic_visit(node)
                self.loop_depth -= 1

            def visit_Call(self, node: ast.Call) -> None:
                if self.loop_depth > 0:
                    func = node.func
                    is_re_call = False
                    # re.compile("..."), re.match("..."), etc.
                    if (isinstance(func, ast.Attribute)
                        and func.attr in _RE_FUNCS
                        and isinstance(func.value, ast.Name)
                        and func.value.id == "re"):
                        # Check first arg is a string literal
                        if node.args and isinstance(node.args[0], ast.Constant) and isinstance(node.args[0].value, str):
                            is_re_call = True
                    if is_re_call:
                        if node.lineno <= len(lines) and "# noqa" in lines[node.lineno - 1]:
                            pass
                        else:
                            issues.append(DoctorIssue(
                                check="performance/regex-in-loop",
                                severity="warning",
                                message=f"re.{func.attr}() with literal pattern inside loop — hoist to module level",
                                path=module.rel_path,
                                category="Performance",
                                help="Compile regex patterns outside loops: PATTERN = re.compile('...') at module level.",
                                line=node.lineno,
                            ))
                self.generic_visit(node)

        _LoopVisitor().visit(module.tree)
    return issues

def check_n_plus_one_hint() -> list[DoctorIssue]:
    """Detect potential N+1 query patterns: database calls inside loops.

    Flags calls to session.query(), session.execute(), session.get(),
    .filter(), .all(), .first(), .one() inside for/while loops. These often
    indicate N+1 queries that should be batched with IN clauses or joins.
    """
    _DB_ATTRS = frozenset({"query", "execute", "get", "filter", "filter_by", "all", "first", "one", "scalars", "scalar"})
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if not any(hint in module.source.lower() for hint in ("session", "db", "database", "conn", "connection", "cursor")):
            continue
        lines = module.source.splitlines()

        class _LoopDBVisitor(ast.NodeVisitor):
            def __init__(self) -> None:
                self.loop_target_stack: list[set[str]] = []
                self.seen_lines: set[int] = set()

            def visit_For(self, node: ast.For) -> None:
                loop_names = {
                    child.id
                    for child in ast.walk(node.target)
                    if isinstance(child, ast.Name)
                }
                self.loop_target_stack.append(loop_names)
                self.generic_visit(node)
                self.loop_target_stack.pop()

            def visit_While(self, node: ast.While) -> None:
                condition_names = {
                    child.id
                    for child in ast.walk(node.test)
                    if isinstance(child, ast.Name)
                }
                self.loop_target_stack.append(condition_names)
                self.generic_visit(node)
                self.loop_target_stack.pop()

            def visit_Call(self, node: ast.Call) -> None:
                if self.loop_target_stack and node.lineno not in self.seen_lines:
                    func = node.func
                    if isinstance(func, ast.Attribute) and func.attr in _DB_ATTRS:
                        # Heuristic: the object should look like a session/db variable
                        obj = func.value
                        # Only flag if object name suggests a DB session
                        session_hints = {"session", "db", "database", "conn", "connection", "cursor"}
                        referenced_names = {
                            child.id
                            for child in ast.walk(node)
                            if isinstance(child, ast.Name)
                        }
                        current_loop_names = set().union(*self.loop_target_stack)
                        if (
                            isinstance(obj, ast.Name)
                            and obj.id.lower() in session_hints
                            and referenced_names & current_loop_names
                        ):
                            if node.lineno <= len(lines) and "# noqa" in lines[node.lineno - 1]:
                                pass
                            else:
                                self.seen_lines.add(node.lineno)
                                issues.append(DoctorIssue(
                                    check="performance/n-plus-one-hint",
                                    severity="warning",
                                    message=f"Potential N+1: {obj.id}.{func.attr}() inside loop — batch with IN clause or join",
                                    path=module.rel_path,
                                    category="Performance",
                                    help="Collect IDs first, then query in batch: session.query(M).filter(M.id.in_(ids))",
                                    line=node.lineno,
                                ))
                self.generic_visit(node)

        _LoopDBVisitor().visit(module.tree)
    return issues


def check_heavy_imports() -> list[DoctorIssue]:
    """Top-level imports of heavy libraries degrade serverless cold-start times.

    Heavy libraries (like agno, openai, pandas, etc.) can add seconds to cold-starts.
    Importing them inside the function scope where they are needed ensures they
    are only loaded on-demand.
    """
    heavy_libs = {"agno", "openai", "pandas", "numpy", "torch", "transformers", "playwright", "langchain"}
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        for node in module.tree.body:
            # Only check top-level imports in the module body
            target_libs: set[str] = set()
            if isinstance(node, ast.Import):
                target_libs = {alias.name.split(".")[0] for alias in node.names}
            elif isinstance(node, ast.ImportFrom) and node.module:
                target_libs = {node.module.split(".")[0]}

            found = target_libs & heavy_libs
            if found:
                issues.append(
                    DoctorIssue(
                        check="performance/heavy-imports",
                        severity="warning",
                        message=f"Heavy library {found} imported at module level — degrades serverless cold-starts",
                        path=module.rel_path,
                        category="Performance",
                        help="Move the import inside the function or router handler that uses it (lazy loading).",
                        line=node.lineno,
                    )
                )
    return issues


__all__ = [
    "check_heavy_imports",
    "check_n_plus_one_hint",
    "check_regex_in_loop",
    "check_sequential_awaits",
]
