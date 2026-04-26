use rustpython_parser::ast::{self, Expr, Ranged, Stmt};
use std::collections::HashSet;

use fastapi_doctor_core::ast_helpers::*;
use fastapi_doctor_core::{Issue, ModuleIndex};

pub(crate) fn collect_sync_io_in_async_issues(
    module: &ModuleIndex,
    function_index: &FunctionIndex,
) -> Vec<Issue> {
    let mut issues = Vec::new();
    let mut emitted_transitive = HashSet::new();

    for function in &function_index.functions {
        if !function.is_async {
            continue;
        }

        for call in &function.direct_calls {
            if let Some((label, help)) = blocking_call_details(call) {
                if module.is_rule_suppressed(call.line, "correctness/sync-io-in-async") {
                    continue;
                }
                issues.push(Issue {
                    check: "correctness/sync-io-in-async",
                    severity: "error",
                    category: "Correctness",
                    line: call.line,
                    path: module.rel_path.to_string(),
                    message: Box::leak(
                        format!(
                            "{label} inside async function '{}' blocks the event loop",
                            function.qualname
                        )
                        .into_boxed_str(),
                    ),
                    help: Box::leak(help.to_string().into_boxed_str()),
                });
                continue;
            }

            let Some(resolved) = function_index.resolve_call(function, call) else {
                continue;
            };
            if resolved.is_async {
                continue;
            }

            let Some((nested_label, _nested_help)) = find_blocking_call_in_sync_helper(
                &resolved.qualname,
                function_index,
                0,
                &mut HashSet::new(),
            ) else {
                continue;
            };

            let fingerprint = format!("{}:{}:{}", function.qualname, call.line, resolved.qualname);
            if emitted_transitive.contains(&fingerprint)
                || module.is_rule_suppressed(call.line, "correctness/sync-io-in-async")
            {
                continue;
            }
            emitted_transitive.insert(fingerprint);
            issues.push(Issue {
                check: "correctness/sync-io-in-async",
                severity: "error",
                category: "Correctness",
                line: call.line,
                path: module.rel_path.to_string(),
                message: Box::leak(
                    format!(
                        "Async function '{}' calls sync helper '{}' that blocks the event loop",
                        function.qualname, resolved.qualname
                    )
                    .into_boxed_str(),
                ),
                help: Box::leak(
                    format!(
                        "Convert '{}()' to non-blocking async work or run it in a thread. Blocking path detected via {}.",
                        resolved.qualname, nested_label
                    )
                    .into_boxed_str(),
                ),
            });
        }
    }

    issues
}

pub(crate) fn collect_misused_async_construct_issues(
    module: &ModuleIndex,
    function_index: &FunctionIndex,
) -> Vec<Issue> {
    let mut issues = Vec::new();

    for function in &function_index.functions {
        if !function.is_async {
            continue;
        }

        for call in &function.await_calls {
            let Some(resolved) = function_index.resolve_call(function, call) else {
                continue;
            };
            if !resolved.is_async && !resolved.returns_awaitable {
                issues.push(Issue {
                    check: "correctness/await-on-sync",
                    severity: "error",
                    category: "Correctness",
                    line: call.line,
                    path: module.rel_path.to_string(),
                    message: Box::leak(
                        format!("await used on sync function '{}()'", resolved.qualname).into_boxed_str(),
                    ),
                    help: "await only works on coroutines. Remove 'await' or convert the function to 'async def'.",
                });
            }
        }

        for call in &function.async_for_calls {
            let Some(resolved) = function_index.resolve_call(function, call) else {
                continue;
            };
            if !resolved.is_async && !resolved.is_generator {
                issues.push(Issue {
                    check: "correctness/sync-iterable-in-async-for",
                    severity: "error",
                    category: "Correctness",
                    line: call.line,
                    path: module.rel_path.to_string(),
                    message: Box::leak(
                        format!(
                            "async for used on sync iterable from '{}()'",
                            resolved.qualname
                        )
                        .into_boxed_str(),
                    ),
                    help: "async for requires an async iterator. Use plain 'for' or make the helper an async generator.",
                });
            }
        }

        for call in &function.async_with_calls {
            let Some(resolved) = function_index.resolve_call(function, call) else {
                continue;
            };
            if !resolved.is_async && resolved.is_sync_context_manager {
                issues.push(Issue {
                    check: "correctness/sync-cm-in-async-with",
                    severity: "error",
                    category: "Correctness",
                    line: call.line,
                    path: module.rel_path.to_string(),
                    message: Box::leak(
                        format!(
                            "async with used on sync context manager from '{}()'",
                            resolved.qualname
                        )
                        .into_boxed_str(),
                    ),
                    help: "async with requires an async context manager. Use plain 'with' or @asynccontextmanager.",
                });
            }
        }
    }

    issues
}

pub(crate) fn collect_asyncio_run_in_async_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if module.file_name.as_deref() == Some("__main__.py")
        || module.file_name.as_deref() == Some("cli.py")
        || module.rel_path.contains("scripts/")
        || !module.source.contains("asyncio")
        || !module_has_async_def(suite)
    {
        return Vec::new();
    }

    let main_ranges = name_main_line_ranges(module, suite);
    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else {
            return;
        };
        let Expr::Attribute(func) = &*call.func else {
            return;
        };
        let Expr::Name(base) = &*func.value else {
            return;
        };
        if base.id.as_str() != "asyncio" || func.attr.as_str() != "run" {
            return;
        }
        let line = module.line_for_offset(call.range.start().to_usize());
        if main_ranges
            .iter()
            .any(|(start, end)| *start <= line && line <= *end)
            || module.is_rule_suppressed(line, "correctness/asyncio-run-in-async")
        {
            return;
        }
        issues.push(Issue {
            check: "correctness/asyncio-run-in-async",
            severity: "error",
            category: "Correctness",
            line,
            path: module.rel_path.to_string(),
            message:
                "asyncio.run() in a module with async functions — use await or create_task instead",
            help:
                "asyncio.run() creates a new loop and blocks. In async code, use 'await' directly.",
        });
    });
    issues
}

pub(crate) fn collect_threading_lock_in_async_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if !module.source.contains("Lock") || !module_has_async_def(suite) {
        return Vec::new();
    }

    let imported_threading_lock = suite_imports_threading_lock(suite);
    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else {
            return;
        };
        let is_threading_lock = match &*call.func {
            Expr::Attribute(func) => matches!(
                (&*func.value, func.attr.as_str()),
                (Expr::Name(base), "Lock") if base.id.as_str() == "threading"
            ),
            Expr::Name(func) => imported_threading_lock && func.id.as_str() == "Lock",
            _ => false,
        };
        if !is_threading_lock {
            return;
        }
        let line = module.line_for_offset(call.range.start().to_usize());
        if module.is_rule_suppressed(line, "correctness/threading-lock-in-async") {
            return;
        }
        issues.push(Issue {
            check: "correctness/threading-lock-in-async",
            severity: "warning",
            category: "Correctness",
            line,
            path: module.rel_path.to_string(),
            message: "threading.Lock() in async module — blocks event loop; use asyncio.Lock()",
            help: "threading.Lock blocks the event loop. Use asyncio.Lock for async code, or add '# noqa' if cross-thread sync is intentional.",
        });
    });
    issues
}

pub(crate) fn collect_mutable_default_arg_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    let mut issues = Vec::new();
    for function in all_functions(suite) {
        for default in &function.defaults {
            if !is_mutable_default(default) {
                continue;
            }
            let line = module.line_for_offset(default.range().start().to_usize());
            if module.is_rule_suppressed(line, "correctness/mutable-default-arg") {
                continue;
            }
            issues.push(Issue {
                check: "correctness/mutable-default-arg",
                severity: "error",
                category: "Correctness",
                line,
                path: module.rel_path.to_string(),
                message: Box::leak(
                    format!(
                        "Mutable default argument in {}() — shared across calls",
                        function.name
                    )
                    .into_boxed_str(),
                ),
                help: "Use None as default: def foo(items=None): items = items or []",
            });
        }
    }
    issues
}

pub(crate) fn collect_import_time_default_call_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    let mut issues = Vec::new();

    for stmt in suite {
        let Stmt::ClassDef(class_node) = stmt else {
            continue;
        };
        if !is_dataclass_node(class_node) && !is_pydantic_model_node(class_node) {
            continue;
        }

        for body_stmt in &class_node.body {
            let Stmt::AnnAssign(ann) = body_stmt else {
                continue;
            };
            let Some(value) = ann.value.as_ref() else {
                continue;
            };
            if !looks_like_import_time_factory_call(value) || is_default_factory_call(value) {
                continue;
            }

            let line = module.line_for_offset(ann.range.start().to_usize());
            if module.is_rule_suppressed(line, "correctness/import-time-default-call") {
                continue;
            }

            let field_name = match &*ann.target {
                Expr::Name(name) => name.id.as_str(),
                _ => "field",
            };
            issues.push(Issue {
                check: "correctness/import-time-default-call",
                severity: "warning",
                category: "Correctness",
                line,
                path: module.rel_path.to_string(),
                message: Box::leak(
                    format!(
                        "Field '{}' in class '{}' calls a factory at import time",
                        field_name, class_node.name
                    )
                    .into_boxed_str(),
                ),
                help: "Use field(default_factory=...) or Field(default_factory=...) so each instance gets a fresh value.",
            });
        }
    }

    issues
}

pub(crate) fn collect_exposed_mutable_state_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    let mut issues = Vec::new();

    for stmt in suite {
        let Stmt::ClassDef(class_node) = stmt else {
            continue;
        };

        let mutable_attrs = collect_mutable_attr_names(class_node);
        if mutable_attrs.is_empty() {
            continue;
        }

        for body_stmt in &class_node.body {
            let (name, body, decorators, range, args) = match body_stmt {
                Stmt::FunctionDef(node) => (
                    node.name.as_str(),
                    &node.body,
                    &node.decorator_list,
                    node.range(),
                    &node.args,
                ),
                Stmt::AsyncFunctionDef(node) => (
                    node.name.as_str(),
                    &node.body,
                    &node.decorator_list,
                    node.range(),
                    &node.args,
                ),
                _ => continue,
            };

            if !is_getter_like_method(name, decorators, args) {
                continue;
            }

            let Some(returned_attr) = returned_self_attr_directly(body) else {
                continue;
            };
            if !mutable_attrs.contains(returned_attr) {
                continue;
            }

            let line = module.line_for_offset(range.start().to_usize());
            if module.is_rule_suppressed(line, "correctness/exposed-mutable-state") {
                continue;
            }

            issues.push(Issue {
                check: "correctness/exposed-mutable-state",
                severity: "warning",
                category: "Correctness",
                line,
                path: module.rel_path.to_string(),
                message: Box::leak(
                    format!(
                        "Method '{}.{}()' returns internal mutable state directly",
                        class_node.name, name
                    )
                    .into_boxed_str(),
                ),
                help: "Return a copy or immutable view instead of exposing the internal collection directly.",
            });
        }
    }

    issues
}

pub(crate) fn collect_return_in_finally_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    let mut issues = Vec::new();
    walk_suite_stmts(suite, &mut |stmt| {
        let finalbody = match stmt {
            Stmt::Try(node) => Some(&node.finalbody),
            Stmt::TryStar(node) => Some(&node.finalbody),
            _ => None,
        };
        let Some(finalbody) = finalbody else {
            return;
        };
        for return_stmt in collect_returns_in_block(finalbody) {
            let line = module.line_for_offset(return_stmt.range().start().to_usize());
            if module.is_rule_suppressed(line, "correctness/return-in-finally") {
                continue;
            }
            issues.push(Issue {
                check: "correctness/return-in-finally",
                severity: "error",
                category: "Correctness",
                line,
                path: module.rel_path.to_string(),
                message: "return inside finally block — silently swallows exceptions",
                help: "Move the return outside the finally block. finally should only do cleanup.",
            });
        }
    });
    issues
}

pub(crate) fn collect_unreachable_code_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    let mut issues = Vec::new();
    walk_suite_stmts(suite, &mut |stmt| match stmt {
        Stmt::FunctionDef(node) => collect_unreachable_in_block(module, &node.body, &mut issues),
        Stmt::AsyncFunctionDef(node) => {
            collect_unreachable_in_block(module, &node.body, &mut issues)
        }
        Stmt::If(node) => {
            collect_unreachable_in_block(module, &node.body, &mut issues);
            collect_unreachable_in_block(module, &node.orelse, &mut issues);
        }
        Stmt::For(node) => {
            collect_unreachable_in_block(module, &node.body, &mut issues);
            collect_unreachable_in_block(module, &node.orelse, &mut issues);
        }
        Stmt::While(node) => {
            collect_unreachable_in_block(module, &node.body, &mut issues);
            collect_unreachable_in_block(module, &node.orelse, &mut issues);
        }
        Stmt::With(node) => collect_unreachable_in_block(module, &node.body, &mut issues),
        Stmt::AsyncWith(node) => collect_unreachable_in_block(module, &node.body, &mut issues),
        Stmt::AsyncFor(node) => {
            collect_unreachable_in_block(module, &node.body, &mut issues);
            collect_unreachable_in_block(module, &node.orelse, &mut issues);
        }
        Stmt::Try(node) => {
            collect_unreachable_in_block(module, &node.body, &mut issues);
            for handler in &node.handlers {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                collect_unreachable_in_block(module, &handler.body, &mut issues);
            }
            collect_unreachable_in_block(module, &node.orelse, &mut issues);
            collect_unreachable_in_block(module, &node.finalbody, &mut issues);
        }
        Stmt::TryStar(node) => {
            collect_unreachable_in_block(module, &node.body, &mut issues);
            for handler in &node.handlers {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                collect_unreachable_in_block(module, &handler.body, &mut issues);
            }
            collect_unreachable_in_block(module, &node.orelse, &mut issues);
            collect_unreachable_in_block(module, &node.finalbody, &mut issues);
        }
        _ => {}
    });
    issues
}

pub(crate) fn collect_untracked_background_task_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if !module.source.contains("create_task") {
        return Vec::new();
    }

    fn is_create_task_call(expr: &Expr) -> bool {
        let Expr::Call(call) = expr else {
            return false;
        };
        match &*call.func {
            Expr::Attribute(attr) => {
                matches!(&*attr.value, Expr::Name(base) if base.id.as_str() == "asyncio")
                    && attr.attr.as_str() == "create_task"
            }
            Expr::Name(name) => name.id.as_str() == "create_task",
            _ => false,
        }
    }

    let mut issues = Vec::new();
    walk_suite_stmts(suite, &mut |stmt| {
        let Stmt::Expr(expr_stmt) = stmt else {
            return;
        };
        if !is_create_task_call(&expr_stmt.value) {
            return;
        }
        let line = module.line_for_offset(expr_stmt.range.start().to_usize());
        if module.is_rule_suppressed(line, "correctness/untracked-background-task") {
            return;
        }
        issues.push(Issue {
            check: "correctness/untracked-background-task",
            severity: "warning",
            category: "Correctness",
            line,
            path: module.rel_path.to_string(),
            message: "asyncio.create_task() result is not retained or supervised",
            help: "Store the task, attach error handling, or run it through FastAPI/Starlette background task infrastructure so failures are visible.",
        });
    });
    issues
}

fn find_blocking_call_in_sync_helper(
    helper_name: &str,
    function_index: &FunctionIndex,
    depth: usize,
    seen: &mut HashSet<String>,
) -> Option<(&'static str, &'static str)> {
    if depth >= ASYNC_HELPER_MAX_DEPTH || seen.contains(helper_name) {
        return None;
    }
    seen.insert(helper_name.to_string());

    let helper = function_index.get_context(helper_name)?;
    for call in &helper.direct_calls {
        if let Some(blocking) = blocking_call_details(call) {
            return Some(blocking);
        }
        let Some(resolved) = function_index.resolve_call(helper, call) else {
            continue;
        };
        if resolved.is_async {
            continue;
        }
        let mut nested_seen = seen.clone();
        if let Some(blocking) = find_blocking_call_in_sync_helper(
            &resolved.qualname,
            function_index,
            depth + 1,
            &mut nested_seen,
        ) {
            return Some(blocking);
        }
    }

    None
}

fn blocking_call_details(call: &CallSite) -> Option<(&'static str, &'static str)> {
    match &call.callee {
        Callee::Name(name) if name == "open" => Some((
            "Sync I/O call 'open()'",
            "Use aiofiles.open() or run the file operation in a thread with asyncio.to_thread().",
        )),
        Callee::Attribute {
            base: Some(base),
            attr,
        } if base == "os" && matches!(attr.as_str(), "open" | "scandir" | "listdir") => Some((
            Box::leak(format!("Sync filesystem call 'os.{attr}()'").into_boxed_str()),
            "Use asyncio.to_thread() or an async-friendly filesystem abstraction for blocking os.* calls.",
        )),
        Callee::Attribute {
            base: Some(base),
            attr,
        } if base == "fcntl" && attr == "flock" => Some((
            "Blocking file-lock call 'fcntl.flock()'",
            "Run file locking in asyncio.to_thread() so the event loop stays responsive.",
        )),
        Callee::Attribute {
            base: Some(base),
            attr,
        } if base == "tempfile" && attr == "NamedTemporaryFile" => Some((
            "Sync tempfile call 'tempfile.NamedTemporaryFile()'",
            "Create temp files in a thread or use a non-blocking async workflow around tempfile usage.",
        )),
        Callee::Name(name) if name == "sleep" => Some((
            "Sync I/O call 'sleep()'",
            "Use asyncio.sleep() instead of time.sleep() or a sync sleep wrapper.",
        )),
        Callee::Attribute {
            base: Some(base),
            attr,
        } if base == "time" && attr == "sleep" => {
            Some(("time.sleep()", "Use asyncio.sleep() instead."))
        }
        Callee::Attribute {
            base: Some(base),
            attr,
        } if base == "requests" && SYNC_HTTP_ATTRS.contains(&attr.as_str()) => Some((
            Box::leak(format!("Sync HTTP call 'requests.{attr}()'").into_boxed_str()),
            "Use httpx.AsyncClient or aiohttp instead of the requests library.",
        )),
        Callee::Attribute {
            base: Some(base),
            attr,
        } if base == "subprocess"
            && matches!(
                attr.as_str(),
                "run" | "Popen" | "call" | "check_call" | "check_output"
            ) =>
        {
            Some((
                Box::leak(format!("Blocking process call 'subprocess.{attr}()'").into_boxed_str()),
                "Use asyncio.create_subprocess_exec() or run the process call in asyncio.to_thread().",
            ))
        }
        Callee::Attribute { attr, .. }
            if matches!(
                attr.as_str(),
                "open" | "read_text" | "read_bytes" | "write_text" | "write_bytes"
            ) =>
        {
            Some((
                Box::leak(format!("Blocking filesystem method '{}()'", attr).into_boxed_str()),
                "Path/file methods are synchronous. Use aiofiles or asyncio.to_thread() in async code.",
            ))
        }
        _ => None,
    }
}

pub(crate) fn collect_returns_in_block<'a>(body: &'a [Stmt]) -> Vec<&'a ast::StmtReturn> {
    let mut returns = Vec::new();
    for stmt in body {
        collect_returns_in_stmt(stmt, &mut returns);
    }
    returns
}

pub(crate) fn collect_returns_in_stmt<'a>(stmt: &'a Stmt, returns: &mut Vec<&'a ast::StmtReturn>) {
    match stmt {
        Stmt::Return(node) => returns.push(node),
        Stmt::FunctionDef(_) | Stmt::AsyncFunctionDef(_) | Stmt::ClassDef(_) => {}
        Stmt::For(node) => {
            for inner in &node.body {
                collect_returns_in_stmt(inner, returns);
            }
            for inner in &node.orelse {
                collect_returns_in_stmt(inner, returns);
            }
        }
        Stmt::AsyncFor(node) => {
            for inner in &node.body {
                collect_returns_in_stmt(inner, returns);
            }
            for inner in &node.orelse {
                collect_returns_in_stmt(inner, returns);
            }
        }
        Stmt::While(node) => {
            for inner in &node.body {
                collect_returns_in_stmt(inner, returns);
            }
            for inner in &node.orelse {
                collect_returns_in_stmt(inner, returns);
            }
        }
        Stmt::If(node) => {
            for inner in &node.body {
                collect_returns_in_stmt(inner, returns);
            }
            for inner in &node.orelse {
                collect_returns_in_stmt(inner, returns);
            }
        }
        Stmt::With(node) => {
            for inner in &node.body {
                collect_returns_in_stmt(inner, returns);
            }
        }
        Stmt::AsyncWith(node) => {
            for inner in &node.body {
                collect_returns_in_stmt(inner, returns);
            }
        }
        Stmt::Try(node) => {
            for inner in &node.body {
                collect_returns_in_stmt(inner, returns);
            }
            for handler in &node.handlers {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                for inner in &handler.body {
                    collect_returns_in_stmt(inner, returns);
                }
            }
            for inner in &node.orelse {
                collect_returns_in_stmt(inner, returns);
            }
            for inner in &node.finalbody {
                collect_returns_in_stmt(inner, returns);
            }
        }
        Stmt::TryStar(node) => {
            for inner in &node.body {
                collect_returns_in_stmt(inner, returns);
            }
            for handler in &node.handlers {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                for inner in &handler.body {
                    collect_returns_in_stmt(inner, returns);
                }
            }
            for inner in &node.orelse {
                collect_returns_in_stmt(inner, returns);
            }
            for inner in &node.finalbody {
                collect_returns_in_stmt(inner, returns);
            }
        }
        Stmt::Match(node) => {
            for case in &node.cases {
                for inner in &case.body {
                    collect_returns_in_stmt(inner, returns);
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn collect_unreachable_in_block(
    module: &ModuleIndex,
    body: &[Stmt],
    issues: &mut Vec<Issue>,
) {
    for (idx, stmt) in body.iter().enumerate() {
        if !is_terminal_stmt(stmt) || idx + 1 >= body.len() {
            continue;
        }
        let next_stmt = &body[idx + 1];
        if matches!(
            next_stmt,
            Stmt::Expr(expr)
                if matches!(&*expr.value, Expr::Constant(constant) if matches!(constant.value, ast::Constant::Str(_)))
        ) {
            continue;
        }
        let line = module.line_for_offset(next_stmt.range().start().to_usize());
        if module.is_rule_suppressed(line, "correctness/unreachable-code") {
            continue;
        }
        issues.push(Issue {
            check: "correctness/unreachable-code",
            severity: "warning",
            category: "Correctness",
            line,
            path: module.rel_path.to_string(),
            message: Box::leak(
                format!(
                    "Unreachable code after {} statement",
                    terminal_stmt_name(stmt)
                )
                .into_boxed_str(),
            ),
            help: "This code never executes. Remove it or fix the control flow logic.",
        });
        break;
    }
}

fn is_terminal_stmt(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::Return(_) | Stmt::Raise(_) | Stmt::Break(_) | Stmt::Continue(_)
    )
}

fn terminal_stmt_name(stmt: &Stmt) -> &'static str {
    match stmt {
        Stmt::Return(_) => "return",
        Stmt::Raise(_) => "raise",
        Stmt::Break(_) => "break",
        Stmt::Continue(_) => "continue",
        _ => "terminal",
    }
}

pub(crate) fn collect_serverless_filesystem_write_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if module.rel_path.contains("scripts/") || module.rel_path.contains("tests/") {
        return Vec::new();
    }
    let write_methods: HashSet<&str> = [
        "write_text",
        "write_bytes",
        "mkdir",
        "touch",
        "rename",
        "replace",
    ]
    .into_iter()
    .collect();
    let base_safe_temp_names = collect_safe_temp_path_names(suite, &HashSet::new());
    let safe_temp_helper_names = collect_safe_temp_helper_names(suite, &base_safe_temp_names);
    let safe_temp_names = collect_safe_temp_path_names(suite, &safe_temp_helper_names);
    let base_path_like_names = collect_path_like_names(suite, &safe_temp_names, &HashSet::new());
    let path_like_helper_names = collect_path_like_helper_names(
        suite,
        &safe_temp_names,
        &base_path_like_names,
        &safe_temp_helper_names,
    );
    let path_like_names = collect_path_like_names(suite, &safe_temp_names, &path_like_helper_names);
    let function_param_scopes = collect_function_param_scopes(suite);
    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        let mut is_write = false;
        let mut write_target = None;
        match &*call.func {
            Expr::Name(n) if n.id.as_str() == "open" => {
                // Check mode arg
                if call.args.len() > 1 {
                    if let Expr::Constant(c) = &call.args[1] {
                        if let ast::Constant::Str(mode) = &c.value {
                            if mode.contains('w') || mode.contains('a') || mode.contains('x') {
                                is_write = true;
                            }
                        }
                    }
                }
                if let Some(first_arg) = call.args.first() {
                    write_target = Some(first_arg);
                }
                for kw in &call.keywords {
                    if kw.arg.as_deref() == Some("mode") {
                        if let Expr::Constant(c) = &kw.value {
                            if let ast::Constant::Str(mode) = &c.value {
                                if mode.contains('w') || mode.contains('a') || mode.contains('x') {
                                    is_write = true;
                                }
                            }
                        }
                    }
                }
            }
            Expr::Name(n)
                if matches!(
                    n.id.as_str(),
                    "atomic_write_text"
                        | "atomic_write_bytes"
                        | "ensure_directory"
                        | "ensure_parent_directory"
                ) =>
            {
                is_write = true;
                if let Some(first_arg) = call.args.first() {
                    write_target = Some(first_arg);
                }
            }
            Expr::Attribute(a) if matches!((&*a.value, a.attr.as_str()), (Expr::Name(base), "makedirs") if base.id.as_str() == "os") =>
            {
                is_write = true;
                if let Some(first_arg) = call.args.first() {
                    write_target = Some(first_arg);
                }
            }
            Expr::Attribute(a)
                if write_methods.contains(a.attr.as_str())
                    && expr_is_path_like(
                        &a.value,
                        &safe_temp_names,
                        &path_like_names,
                        &path_like_helper_names,
                    ) =>
            {
                is_write = true;
                write_target = Some(&a.value);
            }
            _ => {}
        }
        if is_write {
            if write_target.is_some_and(|target| {
                expr_is_safe_temp_path(target, &safe_temp_names, &safe_temp_helper_names)
            }) {
                return;
            }
            if write_target.is_some_and(|target| {
                target_depends_on_function_param(
                    target,
                    call.range.start().to_usize(),
                    &function_param_scopes,
                )
            }) {
                return;
            }
            let line = module.line_for_offset(call.range.start().to_usize());
            issues.push(Issue {
                check: "correctness/serverless-filesystem-write",
                severity: "warning",
                category: "Correctness",
                line,
                path: module.rel_path.to_string(),
                message: "Potential filesystem write outside /tmp — will fail in serverless environments",
                help: "Use /tmp for temporary storage or an external object store (S3/GCS) for persistence.",
            });
        }
    });
    issues
}

fn collect_safe_temp_helper_names(
    suite: &ast::Suite,
    safe_temp_names: &HashSet<String>,
) -> HashSet<String> {
    let mut helper_names = HashSet::new();
    loop {
        let mut changed = false;
        walk_suite_stmts(suite, &mut |stmt| match stmt {
            Stmt::FunctionDef(node) => {
                if function_returns_safe_temp_path(&node.body, safe_temp_names, &helper_names) {
                    changed |= helper_names.insert(node.name.to_string());
                }
            }
            Stmt::AsyncFunctionDef(node) => {
                if function_returns_safe_temp_path(&node.body, safe_temp_names, &helper_names) {
                    changed |= helper_names.insert(node.name.to_string());
                }
            }
            _ => {}
        });
        if !changed {
            break;
        }
    }
    helper_names
}

fn collect_path_like_helper_names(
    suite: &ast::Suite,
    safe_temp_names: &HashSet<String>,
    path_like_names: &HashSet<String>,
    safe_temp_helper_names: &HashSet<String>,
) -> HashSet<String> {
    let mut helper_names = safe_temp_helper_names.clone();
    loop {
        let mut changed = false;
        walk_suite_stmts(suite, &mut |stmt| match stmt {
            Stmt::FunctionDef(node) => {
                if function_returns_path_like(
                    &node.body,
                    safe_temp_names,
                    path_like_names,
                    safe_temp_helper_names,
                    &helper_names,
                ) {
                    changed |= helper_names.insert(node.name.to_string());
                }
            }
            Stmt::AsyncFunctionDef(node) => {
                if function_returns_path_like(
                    &node.body,
                    safe_temp_names,
                    path_like_names,
                    safe_temp_helper_names,
                    &helper_names,
                ) {
                    changed |= helper_names.insert(node.name.to_string());
                }
            }
            _ => {}
        });
        if !changed {
            break;
        }
    }
    helper_names
}

fn collect_safe_temp_path_names(
    suite: &ast::Suite,
    safe_temp_helper_names: &HashSet<String>,
) -> HashSet<String> {
    let mut safe_names = HashSet::new();

    loop {
        let mut changed = false;
        walk_suite_stmts(suite, &mut |stmt| {
            let (targets, value) = match stmt {
                Stmt::Assign(node) => (node.targets.as_slice(), Some(&*node.value)),
                Stmt::AnnAssign(node) => {
                    if let Some(value) = node.value.as_deref() {
                        (std::slice::from_ref(&*node.target), Some(value))
                    } else {
                        (&[][..], None)
                    }
                }
                _ => (&[][..], None),
            };
            let Some(value) = value else {
                return;
            };
            if !expr_is_safe_temp_path(value, &safe_names, safe_temp_helper_names) {
                return;
            }
            for target in targets {
                if let Expr::Name(name) = target {
                    changed |= safe_names.insert(name.id.to_string());
                }
            }
        });
        if !changed {
            break;
        }
    }

    safe_names
}

fn collect_path_like_names(
    suite: &ast::Suite,
    safe_temp_names: &HashSet<String>,
    path_like_helper_names: &HashSet<String>,
) -> HashSet<String> {
    let mut path_like_names = safe_temp_names.clone();

    loop {
        let mut changed = false;
        walk_suite_stmts(suite, &mut |stmt| {
            let (targets, value) = match stmt {
                Stmt::Assign(node) => (node.targets.as_slice(), Some(&*node.value)),
                Stmt::AnnAssign(node) => {
                    if let Some(value) = node.value.as_deref() {
                        (std::slice::from_ref(&*node.target), Some(value))
                    } else {
                        (&[][..], None)
                    }
                }
                _ => (&[][..], None),
            };
            let Some(value) = value else {
                return;
            };
            if !expr_is_path_like(
                value,
                safe_temp_names,
                &path_like_names,
                path_like_helper_names,
            ) {
                return;
            }
            for target in targets {
                if let Expr::Name(name) = target {
                    changed |= path_like_names.insert(name.id.to_string());
                }
            }
        });
        if !changed {
            break;
        }
    }

    path_like_names
}

fn function_returns_safe_temp_path(
    body: &[Stmt],
    safe_temp_names: &HashSet<String>,
    helper_names: &HashSet<String>,
) -> bool {
    body.iter().any(|stmt| {
        if let Stmt::Return(ret) = stmt {
            if let Some(value) = &ret.value {
                return expr_is_safe_temp_path(value, safe_temp_names, helper_names);
            }
        }
        false
    })
}

fn function_returns_path_like(
    body: &[Stmt],
    safe_temp_names: &HashSet<String>,
    path_like_names: &HashSet<String>,
    safe_temp_helper_names: &HashSet<String>,
    path_like_helper_names: &HashSet<String>,
) -> bool {
    body.iter().any(|stmt| {
        if let Stmt::Return(ret) = stmt {
            if let Some(value) = &ret.value {
                return expr_is_path_like(
                    value,
                    safe_temp_names,
                    path_like_names,
                    path_like_helper_names,
                ) || expr_is_safe_temp_path(value, safe_temp_names, safe_temp_helper_names);
            }
        }
        false
    })
}

fn expr_is_safe_temp_path(
    expr: &Expr,
    safe_names: &HashSet<String>,
    safe_temp_helper_names: &HashSet<String>,
) -> bool {
    match expr {
        Expr::Name(node) => safe_names.contains(node.id.as_str()),
        Expr::Constant(node) => matches!(
            &node.value,
            ast::Constant::Str(value) if value == "/tmp" || value.starts_with("/tmp/")
        ),
        Expr::Attribute(node) => {
            expr_is_safe_temp_path(&node.value, safe_names, safe_temp_helper_names)
        }
        Expr::Call(node) => call_returns_safe_temp_path(node, safe_names, safe_temp_helper_names),
        Expr::BinOp(node) => {
            matches!(&node.op, ast::Operator::Div)
                && expr_is_safe_temp_path(&node.left, safe_names, safe_temp_helper_names)
        }
        Expr::IfExp(node) => {
            (expr_is_safe_temp_path(&node.orelse, safe_names, safe_temp_helper_names)
                && guard_implies_safe_temp(&node.test, &node.body))
                || (expr_is_safe_temp_path(&node.body, safe_names, safe_temp_helper_names)
                    && guard_implies_safe_temp(&node.test, &node.orelse))
        }
        _ => false,
    }
}

fn expr_is_path_like(
    expr: &Expr,
    safe_temp_names: &HashSet<String>,
    path_like_names: &HashSet<String>,
    path_like_helper_names: &HashSet<String>,
) -> bool {
    match expr {
        Expr::Name(node) => {
            safe_temp_names.contains(node.id.as_str()) || path_like_names.contains(node.id.as_str())
        }
        Expr::Attribute(node) => {
            matches!(node.attr.as_str(), "parent")
                && expr_is_path_like(
                    &node.value,
                    safe_temp_names,
                    path_like_names,
                    path_like_helper_names,
                )
        }
        Expr::Call(node) => call_returns_path_like(
            node,
            safe_temp_names,
            path_like_names,
            path_like_helper_names,
        ),
        Expr::BinOp(node) => {
            matches!(&node.op, ast::Operator::Div)
                && expr_is_path_like(
                    &node.left,
                    safe_temp_names,
                    path_like_names,
                    path_like_helper_names,
                )
        }
        _ => false,
    }
}

fn call_returns_safe_temp_path(
    call: &ast::ExprCall,
    safe_names: &HashSet<String>,
    safe_temp_helper_names: &HashSet<String>,
) -> bool {
    match &*call.func {
        Expr::Name(name) => {
            match name.id.as_str() {
                "serverless_temp_root" => true,
                "str" => call.args.first().is_some_and(|arg| {
                    expr_is_safe_temp_path(arg, safe_names, safe_temp_helper_names)
                }),
                helper if safe_temp_helper_names.contains(helper) => true,
                "Path" | "PurePath" => call.args.first().is_some_and(|arg| {
                    expr_is_safe_temp_path(arg, safe_names, safe_temp_helper_names)
                }),
                _ => false,
            }
        }
        Expr::Attribute(attr) => {
            let attr_name = attr.attr.as_str();
            if let Expr::Name(base) = &*attr.value {
                if base.id.as_str() == "tempfile"
                    && matches!(
                        attr_name,
                        "gettempdir"
                            | "NamedTemporaryFile"
                            | "TemporaryDirectory"
                            | "mkdtemp"
                            | "mkstemp"
                    )
                {
                    return true;
                }
            }
            matches!(
                attr_name,
                "joinpath" | "resolve" | "absolute" | "with_name" | "with_suffix"
            ) && expr_is_safe_temp_path(&attr.value, safe_names, safe_temp_helper_names)
        }
        _ => false,
    }
}

fn call_returns_path_like(
    call: &ast::ExprCall,
    safe_temp_names: &HashSet<String>,
    path_like_names: &HashSet<String>,
    path_like_helper_names: &HashSet<String>,
) -> bool {
    if call_returns_safe_temp_path(call, safe_temp_names, path_like_helper_names) {
        return true;
    }
    match &*call.func {
        Expr::Name(name) => {
            matches!(
                name.id.as_str(),
                "Path" | "PurePath" | "ensure_directory" | "ensure_parent_directory"
            ) || path_like_helper_names.contains(name.id.as_str())
        }
        Expr::Attribute(attr) => {
            if let Expr::Name(base) = &*attr.value {
                if base.id.as_str() == "pathlib"
                    && matches!(attr.attr.as_str(), "Path" | "PurePath")
                {
                    return true;
                }
            }
            matches!(
                attr.attr.as_str(),
                "joinpath" | "resolve" | "absolute" | "with_name" | "with_suffix"
            ) && expr_is_path_like(
                &attr.value,
                safe_temp_names,
                path_like_names,
                path_like_helper_names,
            )
        }
        _ => false,
    }
}

fn guard_implies_safe_temp(test: &Expr, branch_expr: &Expr) -> bool {
    match (test, branch_expr) {
        (Expr::Call(call), Expr::Name(branch_name)) => {
            let Expr::Attribute(attr) = &*call.func else {
                return false;
            };
            if attr.attr.as_str() != "startswith" {
                return false;
            }
            let Some(Expr::Constant(prefix)) = call.args.first() else {
                return false;
            };
            let ast::Constant::Str(prefix) = &prefix.value else {
                return false;
            };
            if prefix != "/tmp" && prefix != "/tmp/" {
                return false;
            }
            match &*attr.value {
                Expr::Call(inner_call) => {
                    let Expr::Name(inner_name) = &*inner_call.func else {
                        return false;
                    };
                    inner_name.id.as_str() == "str"
                        && inner_call.args.first().is_some_and(|arg| {
                            matches!(arg, Expr::Name(name) if name.id.as_str() == branch_name.id.as_str())
                        })
                }
                Expr::Name(name) => name.id.as_str() == branch_name.id.as_str(),
                _ => false,
            }
        }
        _ => false,
    }
}

fn collect_function_param_scopes(suite: &ast::Suite) -> Vec<(usize, usize, HashSet<String>)> {
    let mut scopes = Vec::new();
    walk_suite_stmts(suite, &mut |stmt| match stmt {
        Stmt::FunctionDef(node) => {
            scopes.push((
                node.range.start().to_usize(),
                node.range.end().to_usize(),
                function_param_names(&node.args),
            ));
        }
        Stmt::AsyncFunctionDef(node) => {
            scopes.push((
                node.range.start().to_usize(),
                node.range.end().to_usize(),
                function_param_names(&node.args),
            ));
        }
        _ => {}
    });
    scopes
}

fn function_param_names(args: &ast::Arguments) -> HashSet<String> {
    args.posonlyargs
        .iter()
        .chain(args.args.iter())
        .map(|arg| arg.def.arg.to_string())
        .chain(args.kwonlyargs.iter().map(|arg| arg.def.arg.to_string()))
        .filter(|arg| arg != "self")
        .collect()
}

fn target_depends_on_function_param(
    expr: &Expr,
    offset: usize,
    scopes: &[(usize, usize, HashSet<String>)],
) -> bool {
    let Some(param_names) = scopes
        .iter()
        .filter(|(start, end, _)| *start <= offset && offset <= *end)
        .min_by_key(|(start, end, _)| end - start)
        .map(|(_, _, names)| names)
    else {
        return false;
    };
    expr_depends_on_names(expr, param_names)
}

fn expr_depends_on_names(expr: &Expr, names: &HashSet<String>) -> bool {
    match expr {
        Expr::Name(node) => names.contains(node.id.as_str()),
        Expr::Attribute(node) => expr_depends_on_names(&node.value, names),
        Expr::Call(node) => {
            expr_depends_on_names(&node.func, names)
                || node
                    .args
                    .iter()
                    .any(|arg| expr_depends_on_names(arg, names))
                || node
                    .keywords
                    .iter()
                    .any(|kw| expr_depends_on_names(&kw.value, names))
        }
        Expr::BinOp(node) => {
            expr_depends_on_names(&node.left, names) || expr_depends_on_names(&node.right, names)
        }
        Expr::IfExp(node) => {
            expr_depends_on_names(&node.body, names) || expr_depends_on_names(&node.orelse, names)
        }
        _ => false,
    }
}

// ── Correctness: missing-http-timeout ───────────────────────────────────

pub(crate) fn collect_missing_http_timeout_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if !module.source.contains("requests") && !module.source.contains("httpx") {
        return Vec::new();
    }
    let http_libs: HashSet<&str> = ["requests", "httpx"].into_iter().collect();
    let http_methods: HashSet<&str> = ["get", "post", "put", "patch", "delete", "head", "request"]
        .into_iter()
        .collect();
    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        let is_http_call = match &*call.func {
            Expr::Attribute(a) => {
                if let Expr::Name(base) = &*a.value {
                    http_libs.contains(base.id.as_str()) && http_methods.contains(a.attr.as_str())
                } else {
                    false
                }
            }
            _ => false,
        };
        if !is_http_call {
            return;
        }
        let has_timeout = call
            .keywords
            .iter()
            .any(|kw| kw.arg.as_deref() == Some("timeout"));
        if !has_timeout {
            let line = module.line_for_offset(call.range.start().to_usize());
            issues.push(Issue {
                check: "correctness/missing-http-timeout",
                severity: "warning",
                category: "Correctness",
                line,
                path: module.rel_path.to_string(),
                message: "HTTP call missing timeout — can hang indefinitely",
                help: "Always specify a timeout for HTTP calls to avoid hanging requests.",
            });
        }
    });
    issues
}

// ── Performance: regex-in-loop ──────────────────────────────────────────

pub(crate) fn collect_get_with_side_effect_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if !module.rel_path.contains("routers/")
        && !module.rel_path.contains("routes/")
        && !module.rel_path.contains("api/")
    {
        return Vec::new();
    }
    let mutation_attrs: HashSet<&str> = [
        "add",
        "delete",
        "commit",
        "update",
        "remove",
        "send",
        "post",
        "put",
        "patch",
        "insert",
        "drop",
        "create",
        "save",
        "bulk_save_objects",
        "merge",
        "flush",
    ]
    .into_iter()
    .collect();

    let mut issues = Vec::new();
    for function in all_functions(suite) {
        // Check if it's a GET handler
        let _source = module.source_slice(function.range);
        // Look for @*.get( pattern in the source above the function
        let func_line = module.line_for_offset(function.range.start().to_usize());
        let is_get = if func_line >= 2 {
            // Check preceding lines for decorator
            let dec_start = func_line.saturating_sub(5);
            (dec_start..func_line).any(|l| {
                if l > 0 && l <= module.lines.len() {
                    let trimmed = &module.lines[l - 1].trimmed;
                    trimmed.starts_with("@") && trimmed.contains(".get(")
                } else {
                    false
                }
            })
        } else {
            false
        };
        if !is_get {
            continue;
        }
        // Walk body for mutation calls
        let mut found = false;
        walk_suite_exprs(function.body, &mut |expr| {
            if found {
                return;
            }
            let Expr::Call(call) = expr else { return };
            if let Expr::Attribute(func) = &*call.func {
                if mutation_attrs.contains(func.attr.as_str()) {
                    let line = module.line_for_offset(call.range.start().to_usize());
                    if !module.is_rule_suppressed(line, "correctness/get-with-side-effect") {
                        issues.push(Issue {
                            check: "correctness/get-with-side-effect",
                            severity: "warning",
                            category: "Correctness",
                            line,
                            path: module.rel_path.to_string(),
                            message: Box::leak(
                                format!("GET endpoint {}() calls .{}() — violates REST semantics", function.name, func.attr).into_boxed_str(),
                            ),
                            help: "GET must be safe/idempotent. Move mutations to POST/PUT/DELETE endpoints.",
                        });
                        found = true;
                    }
                }
            }
        });
    }
    issues
}

// ── Architecture: fat-route-handler ─────────────────────────────────────

fn is_dataclass_node(node: &ast::StmtClassDef) -> bool {
    node.decorator_list.iter().any(|dec| match dec {
        Expr::Name(n) => n.id.as_str() == "dataclass",
        Expr::Attribute(a) => a.attr.as_str() == "dataclass",
        Expr::Call(c) => match &*c.func {
            Expr::Name(n) => n.id.as_str() == "dataclass",
            Expr::Attribute(a) => a.attr.as_str() == "dataclass",
            _ => false,
        },
        _ => false,
    })
}

fn is_pydantic_model_node(node: &ast::StmtClassDef) -> bool {
    node.bases.iter().any(|base| match base {
        Expr::Name(n) => n.id.as_str() == "BaseModel",
        Expr::Attribute(a) => a.attr.as_str() == "BaseModel",
        _ => false,
    })
}

fn looks_like_import_time_factory_call(expr: &Expr) -> bool {
    let Expr::Call(call) = expr else {
        return false;
    };
    match &*call.func {
        Expr::Attribute(attr) => {
            let attr_name = attr.attr.as_str();
            match &*attr.value {
                Expr::Name(base) => matches!(
                    (base.id.as_str(), attr_name),
                    ("datetime", "now")
                        | ("datetime", "utcnow")
                        | ("date", "today")
                        | ("time", "time")
                        | ("uuid", "uuid4")
                        | ("uuid", "uuid1")
                ),
                Expr::Attribute(base_attr) => matches!(
                    (base_attr.attr.as_str(), attr_name),
                    ("datetime", "now")
                        | ("datetime", "utcnow")
                        | ("date", "today")
                        | ("time", "time")
                        | ("uuid", "uuid4")
                        | ("uuid", "uuid1")
                ),
                _ => false,
            }
        }
        _ => false,
    }
}

fn is_default_factory_call(expr: &Expr) -> bool {
    let Expr::Call(call) = expr else {
        return false;
    };
    let is_factory_wrapper = matches!(&*call.func, Expr::Name(name) if matches!(name.id.as_str(), "Field" | "field"))
        || matches!(&*call.func, Expr::Attribute(attr) if matches!(attr.attr.as_str(), "Field" | "field"));
    if !is_factory_wrapper {
        return false;
    }
    call.keywords
        .iter()
        .any(|kw| kw.arg.as_deref() == Some("default_factory"))
}

fn is_mutable_collection_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::List(_))
        || matches!(expr, Expr::Dict(_))
        || matches!(expr, Expr::Set(_))
        || matches!(
            expr,
            Expr::Call(call)
                if matches!(&*call.func, Expr::Name(name) if matches!(name.id.as_str(), "list" | "dict" | "set"))
                    && call.keywords.is_empty()
        )
}

fn collect_mutable_attr_names(node: &ast::StmtClassDef) -> HashSet<&str> {
    let mut attrs = HashSet::new();

    for stmt in &node.body {
        match stmt {
            Stmt::AnnAssign(ann) => {
                if let (Expr::Name(name), Some(value)) = (&*ann.target, ann.value.as_ref()) {
                    if is_mutable_collection_expr(value) {
                        attrs.insert(name.id.as_str());
                    }
                }
            }
            Stmt::Assign(assign) => {
                if is_mutable_collection_expr(&assign.value) {
                    for target in &assign.targets {
                        if let Expr::Name(name) = target {
                            attrs.insert(name.id.as_str());
                        }
                    }
                }
            }
            Stmt::FunctionDef(method) => {
                collect_mutable_attrs_from_init(method.name.as_str(), &method.body, &mut attrs)
            }
            Stmt::AsyncFunctionDef(method) => {
                collect_mutable_attrs_from_init(method.name.as_str(), &method.body, &mut attrs)
            }
            _ => {}
        }
    }

    attrs
}

fn collect_mutable_attrs_from_init<'a>(
    method_name: &str,
    body: &'a [Stmt],
    attrs: &mut HashSet<&'a str>,
) {
    if method_name != "__init__" {
        return;
    }
    walk_suite_stmts(body, &mut |stmt| match stmt {
        Stmt::Assign(assign) if is_mutable_collection_expr(&assign.value) => {
            for target in &assign.targets {
                if let Expr::Attribute(attr) = target {
                    if matches!(&*attr.value, Expr::Name(name) if name.id.as_str() == "self") {
                        attrs.insert(attr.attr.as_str());
                    }
                }
            }
        }
        Stmt::AnnAssign(ann) => {
            if let Some(value) = ann.value.as_ref() {
                if is_mutable_collection_expr(value) {
                    if let Expr::Attribute(attr) = &*ann.target {
                        if matches!(&*attr.value, Expr::Name(name) if name.id.as_str() == "self") {
                            attrs.insert(attr.attr.as_str());
                        }
                    }
                }
            }
        }
        _ => {}
    });
}

fn is_getter_like_method(name: &str, decorators: &[Expr], args: &ast::Arguments) -> bool {
    let positional = args.args.len();
    let has_property = decorators.iter().any(|dec| {
        matches!(dec, Expr::Name(name) if name.id.as_str() == "property")
            || matches!(dec, Expr::Attribute(attr) if attr.attr.as_str() == "property")
    });
    has_property || (positional == 1 && (name.starts_with("get_") || !name.starts_with('_')))
}

fn returned_self_attr_directly(body: &[Stmt]) -> Option<&str> {
    let body = strip_leading_docstring(body);
    let [Stmt::Return(ret)] = body else {
        return None;
    };
    let value = ret.value.as_ref()?;
    let Expr::Attribute(attr) = &**value else {
        return None;
    };
    if matches!(&*attr.value, Expr::Name(name) if name.id.as_str() == "self") {
        Some(attr.attr.as_str())
    } else {
        None
    }
}

fn strip_leading_docstring(body: &[Stmt]) -> &[Stmt] {
    if body.first().is_some_and(|stmt| {
        matches!(
            stmt,
            Stmt::Expr(expr)
                if matches!(&*expr.value, Expr::Constant(node) if matches!(node.value, ast::Constant::Str(_)))
        )
    }) {
        &body[1..]
    } else {
        body
    }
}
