use std::collections::HashSet;
use rustpython_parser::ast::{self, Expr, Ranged, Stmt};

use crate::{Issue, ModuleIndex};
use crate::ast_helpers::*;

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
                    path: module.rel_path.clone(),
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
                path: module.rel_path.clone(),
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
                    path: module.rel_path.clone(),
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
                    path: module.rel_path.clone(),
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
                    path: module.rel_path.clone(),
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

pub(crate) fn collect_asyncio_run_in_async_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
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
            path: module.rel_path.clone(),
            message:
                "asyncio.run() in a module with async functions — use await or create_task instead",
            help:
                "asyncio.run() creates a new loop and blocks. In async code, use 'await' directly.",
        });
    });
    issues
}

pub(crate) fn collect_threading_lock_in_async_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
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
            path: module.rel_path.clone(),
            message: "threading.Lock() in async module — blocks event loop; use asyncio.Lock()",
            help: "threading.Lock blocks the event loop. Use asyncio.Lock for async code, or add '# noqa' if cross-thread sync is intentional.",
        });
    });
    issues
}

pub(crate) fn collect_mutable_default_arg_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
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
                path: module.rel_path.clone(),
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

pub(crate) fn collect_return_in_finally_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
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
                path: module.rel_path.clone(),
                message: "return inside finally block — silently swallows exceptions",
                help: "Move the return outside the finally block. finally should only do cleanup.",
            });
        }
    });
    issues
}

pub(crate) fn collect_unreachable_code_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
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

pub(crate) fn collect_unreachable_in_block(module: &ModuleIndex, body: &[Stmt], issues: &mut Vec<Issue>) {
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
            path: module.rel_path.clone(),
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

pub(crate) fn collect_serverless_filesystem_write_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
    if module.rel_path.contains("scripts/") || module.rel_path.contains("tests/") {
        return Vec::new();
    }
    let write_methods: HashSet<&str> = ["write_text", "write_bytes", "mkdir", "touch", "rename", "replace"].into_iter().collect();
    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        let mut is_write = false;
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
            Expr::Attribute(a) if write_methods.contains(a.attr.as_str()) => {
                is_write = true;
            }
            _ => {}
        }
        if is_write {
            // Check if path looks safe (/tmp)
            let source_fragment = module.source_slice(call.range);
            if source_fragment.contains("/tmp") || source_fragment.to_ascii_lowercase().contains("tempfile") {
                return;
            }
            let line = module.line_for_offset(call.range.start().to_usize());
            issues.push(Issue {
                check: "correctness/serverless-filesystem-write",
                severity: "warning",
                category: "Correctness",
                line,
                path: module.rel_path.clone(),
                message: "Potential filesystem write outside /tmp — will fail in serverless environments",
                help: "Use /tmp for temporary storage or an external object store (S3/GCS) for persistence.",
            });
        }
    });
    issues
}

// ── Correctness: missing-http-timeout ───────────────────────────────────

pub(crate) fn collect_missing_http_timeout_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
    if !module.source.contains("requests") && !module.source.contains("httpx") {
        return Vec::new();
    }
    let http_libs: HashSet<&str> = ["requests", "httpx"].into_iter().collect();
    let http_methods: HashSet<&str> = ["get", "post", "put", "patch", "delete", "head", "request"].into_iter().collect();
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
        let has_timeout = call.keywords.iter().any(|kw| kw.arg.as_deref() == Some("timeout"));
        if !has_timeout {
            let line = module.line_for_offset(call.range.start().to_usize());
            issues.push(Issue {
                check: "correctness/missing-http-timeout",
                severity: "warning",
                category: "Correctness",
                line,
                path: module.rel_path.clone(),
                message: "HTTP call missing timeout — can hang indefinitely",
                help: "Always specify a timeout for HTTP calls to avoid hanging requests.",
            });
        }
    });
    issues
}

// ── Performance: regex-in-loop ──────────────────────────────────────────

pub(crate) fn collect_get_with_side_effect_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
    if !module.rel_path.contains("routers/") && !module.rel_path.contains("routes/") && !module.rel_path.contains("api/") {
        return Vec::new();
    }
    let mutation_attrs: HashSet<&str> = [
        "add", "delete", "commit", "update", "remove", "send",
        "post", "put", "patch", "insert", "drop", "create", "save",
        "bulk_save_objects", "merge", "flush",
    ].into_iter().collect();

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
            if found { return; }
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
                            path: module.rel_path.clone(),
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

