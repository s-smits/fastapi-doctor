use std::collections::HashSet;
use rustpython_parser::ast::{self, Expr, Ranged, Stmt};

use crate::{Config, Issue, ModuleIndex};
use crate::ast_helpers::*;

pub(crate) fn collect_async_without_await_issues(
    module: &ModuleIndex,
    function_index: &FunctionIndex,
) -> Vec<Issue> {
    let mut unnecessary_async = HashSet::new();

    for function in &function_index.functions {
        if function.is_async && !function.has_async_constructs {
            unnecessary_async.insert(function.qualname.clone());
        }
    }

    let mut changed = true;
    while changed {
        changed = false;
        for function in &function_index.functions {
            if !function.is_async || unnecessary_async.contains(&function.qualname) {
                continue;
            }
            if function.has_async_for_or_with {
                continue;
            }
            if function.await_calls.is_empty() {
                continue;
            }

            let all_awaits_unnecessary = function.await_calls.iter().all(|call| {
                function_index
                    .resolve_call(function, call)
                    .is_some_and(|resolved| unnecessary_async.contains(&resolved.qualname))
            });

            if all_awaits_unnecessary {
                unnecessary_async.insert(function.qualname.clone());
                changed = true;
            }
        }
    }

    let mut issues = Vec::new();
    for function in &function_index.functions {
        if !unnecessary_async.contains(&function.qualname) {
            continue;
        }

        let (message, help) = if function.is_route_handler {
            (
                format!(
                    "Async route handler '{}' is effectively synchronous — use plain def to avoid blocking the event loop",
                    function.qualname
                ),
                "FastAPI runs plain def endpoints in a thread pool. This handler either has no awaits or only awaits other functions that don't do real async work.".to_string(),
            )
        } else {
            (
                format!(
                    "async def '{}' is effectively synchronous — convert to plain def unless it must maintain an awaitable interface",
                    function.qualname
                ),
                "This function contains no real async work (awaits, async for/with). Reserve async def for truly awaitable operations.".to_string(),
            )
        };

        issues.push(Issue {
            check: "architecture/async-without-await",
            severity: "warning",
            category: "Architecture",
            line: function.line,
            path: module.rel_path.to_string(),
            message: Box::leak(message.into_boxed_str()),
            help: Box::leak(help.into_boxed_str()),
        });
    }

    issues
}

pub(crate) fn collect_giant_function_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
    config: &Config,
) -> Vec<Issue> {
    let mut issues = Vec::new();
    for function in all_functions(suite) {
        let line = module.line_for_offset(function.range.start().to_usize());
        let end_line = module.line_for_offset(function.range.end().to_usize().saturating_sub(1));
        let size = end_line.saturating_sub(line) + 1;
        let source_segment = module.source_slice(function.range);
        if source_segment.contains("# noqa: architecture") {
            continue;
        }

        if config.giant_function_threshold > 0 && size > config.giant_function_threshold {
            issues.push(Issue {
                check: "architecture/giant-function",
                severity: "error",
                category: "Architecture",
                line,
                path: module.rel_path.to_string(),
                message: Box::leak(
                    format!(
                        "Function '{}' is {} lines (>{}) — extract sub-functions",
                        function.name, size, config.giant_function_threshold
                    )
                    .into_boxed_str(),
                ),
                help: "Break into smaller, testable functions. Each should do one thing.",
            });
        } else if config.large_function_threshold > 0 && size > config.large_function_threshold {
            issues.push(Issue {
                check: "architecture/large-function",
                severity: "warning",
                category: "Architecture",
                line,
                path: module.rel_path.to_string(),
                message: Box::leak(
                    format!(
                        "Function '{}' is {} lines (>{}) — consider splitting",
                        function.name, size, config.large_function_threshold
                    )
                    .into_boxed_str(),
                ),
                help: "Functions over 200 lines are harder to maintain and test.",
            });
        }
    }
    issues
}

pub(crate) fn collect_deep_nesting_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
    threshold: usize,
) -> Vec<Issue> {
    let mut issues = Vec::new();
    for function in all_functions(suite) {
        let source_segment = module.source_slice(function.range);
        if source_segment.contains("# noqa: architecture") {
            continue;
        }

        let depth = max_nesting_depth(function.body);
        if depth > threshold {
            issues.push(Issue {
                check: "architecture/deep-nesting",
                severity: "warning",
                category: "Architecture",
                line: module.line_for_offset(function.range.start().to_usize()),
                path: module.rel_path.to_string(),
                message: Box::leak(
                    format!(
                        "Function '{}' has {} levels of nesting (>{}) — extract inner logic",
                        function.name, depth, threshold
                    )
                    .into_boxed_str(),
                ),
                help: "Use early returns or helper functions to flatten control flow and improve readability.",
            });
        }
    }
    issues
}

// =========================================================================
// New check implementations
// =========================================================================

// ── Resilience rules ────────────────────────────────────────────────────

pub(crate) fn collect_avoid_sys_exit_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
    if module.file_name.as_deref() == Some("__main__.py")
        || module.file_name.as_deref() == Some("cli.py")
        || module.rel_path.contains("scripts/")
    {
        return Vec::new();
    }
    if !module.source.contains("sys.exit") && !module.source.contains("exit(") && !module.source.contains("quit(") {
        return Vec::new();
    }
    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        let is_exit = match &*call.func {
            Expr::Attribute(a) => {
                matches!(&*a.value, Expr::Name(n) if n.id.as_str() == "sys") && a.attr.as_str() == "exit"
            }
            Expr::Name(n) => matches!(n.id.as_str(), "exit" | "quit"),
            _ => false,
        };
        if is_exit {
            let line = module.line_for_offset(call.range.start().to_usize());
            issues.push(Issue {
                check: "architecture/avoid-sys-exit",
                severity: "warning",
                category: "Architecture",
                line,
                path: module.rel_path.to_string(),
                message: "sys.exit() or quit() in library code — raise an Exception instead",
                help: "Deep application logic should raise exceptions, not abruptly kill the process.",
            });
        }
    });
    issues
}

// ── Architecture: engine-pool-pre-ping ──────────────────────────────────

pub(crate) fn collect_engine_pool_pre_ping_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        let func_name = match &*call.func {
            Expr::Name(n) => n.id.as_str(),
            Expr::Attribute(a) => a.attr.as_str(),
            _ => return,
        };
        if func_name != "create_engine" && func_name != "create_async_engine" {
            return;
        }
        let has_pre_ping = call.keywords.iter().any(|kw| {
            kw.arg.as_deref() == Some("pool_pre_ping")
                && matches!(&kw.value, Expr::Constant(c) if matches!(c.value, ast::Constant::Bool(true)))
        });
        if !has_pre_ping {
            let line = module.line_for_offset(call.range.start().to_usize());
            issues.push(Issue {
                check: "architecture/engine-pool-pre-ping",
                severity: "warning",
                category: "Architecture",
                line,
                path: module.rel_path.to_string(),
                message: Box::leak(
                    format!("{}() called without pool_pre_ping=True", func_name).into_boxed_str(),
                ),
                help: "Set pool_pre_ping=True to automatically recover from dropped connections.",
            });
        }
    });
    issues
}

// ── Correctness: serverless-filesystem-write ────────────────────────────

pub(crate) fn collect_fat_route_handler_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
    config: &Config,
) -> Vec<Issue> {
    if !module.has_path_part(&["routers"]) || config.fat_route_handler_threshold == 0 {
        return Vec::new();
    }
    let mut issues = Vec::new();
    for function in all_functions(suite) {
        let source = module.source_slice(function.range);
        if source.contains("# noqa: architecture") {
            continue;
        }
        // Check for route decorator
        let func_line = module.line_for_offset(function.range.start().to_usize());
        let is_route = if func_line >= 2 {
            let dec_start = func_line.saturating_sub(5);
            (dec_start..func_line).any(|l| {
                if l > 0 && l <= module.lines.len() {
                    let trimmed = &module.lines[l - 1].trimmed;
                    trimmed.starts_with("@") && (trimmed.contains("router") || trimmed.contains("app"))
                } else {
                    false
                }
            })
        } else {
            false
        };
        if !is_route {
            continue;
        }
        let end_line = module.line_for_offset(function.range.end().to_usize().saturating_sub(1));
        let func_len = end_line.saturating_sub(func_line) + 1;
        if func_len > config.fat_route_handler_threshold {
            issues.push(Issue {
                check: "architecture/fat-route-handler",
                severity: "warning",
                category: "Architecture",
                line: func_line,
                path: module.rel_path.to_string(),
                message: Box::leak(
                    format!(
                        "Route handler '{}' is {} lines — extract business logic to services/",
                        function.name, func_len
                    )
                    .into_boxed_str(),
                ),
                help: Box::leak(
                    format!(
                        "Keep handlers under {} lines. Move logic to a service function.",
                        config.fat_route_handler_threshold
                    )
                    .into_boxed_str(),
                ),
            });
        }
    }
    issues
}

// ── Architecture: passthrough-function ──────────────────────────────────

pub(crate) fn collect_passthrough_function_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
    let mut issues = Vec::new();
    // Only check module-level functions (not methods)
    for stmt in suite {
        let (name, args, body, decorators, range) = match stmt {
            Stmt::FunctionDef(node) => (
                node.name.as_str(),
                &node.args,
                &node.body,
                &node.decorator_list,
                node.range(),
            ),
            Stmt::AsyncFunctionDef(node) => (
                node.name.as_str(),
                &node.args,
                &node.body,
                &node.decorator_list,
                node.range(),
            ),
            _ => continue,
        };
        // Skip decorated functions
        if !decorators.is_empty() {
            continue;
        }
        // Skip functions with docstrings
        if body.first().is_some_and(|s| matches!(s, Stmt::Expr(e) if matches!(&*e.value, Expr::Constant(c) if matches!(c.value, ast::Constant::Str(_))))) {
            continue;
        }
        // Must be single return statement
        if body.len() != 1 {
            continue;
        }
        let Stmt::Return(ret) = &body[0] else { continue };
        let Some(ret_val) = &ret.value else { continue };
        let Expr::Call(call) = &**ret_val else { continue };

        // Get function param names
        let param_names: HashSet<&str> = args.args.iter().map(|a| a.def.arg.as_str()).collect();
        if param_names.len() < 2 {
            continue;
        }

        // Get call argument names
        let mut call_arg_names: HashSet<&str> = HashSet::new();
        for arg in &call.args {
            if let Expr::Name(n) = arg {
                call_arg_names.insert(n.id.as_str());
            } else if let Expr::Starred(s) = arg {
                if let Expr::Name(n) = &*s.value {
                    call_arg_names.insert(n.id.as_str());
                }
            }
        }
        for kw in &call.keywords {
            if let Expr::Name(n) = &kw.value {
                call_arg_names.insert(n.id.as_str());
            }
        }

        if param_names.is_subset(&call_arg_names) || call_arg_names == param_names {
            let line = module.line_for_offset(range.start().to_usize());
            issues.push(Issue {
                check: "architecture/passthrough-function",
                severity: "warning",
                category: "Architecture",
                line,
                path: module.rel_path.to_string(),
                message: Box::leak(
                    format!("Function '{}' is a pure passthrough — consider inlining", name)
                        .into_boxed_str(),
                ),
                help: "This function just delegates to another. Inline it or add a docstring explaining why the wrapper exists.",
            });
        }
    }
    issues
}

// ── Performance: sequential-awaits ──────────────────────────────────────

