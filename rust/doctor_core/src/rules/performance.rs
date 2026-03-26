use std::collections::HashSet;
use rustpython_parser::ast::{self, Expr, Ranged, Stmt};

use crate::{Issue, ModuleIndex};
use crate::ast_helpers::*;

pub(crate) fn collect_regex_in_loop_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
    if !module.source.contains("re.") {
        return Vec::new();
    }
    let re_funcs: HashSet<&str> = ["compile", "match", "search", "findall", "fullmatch", "sub", "split"].into_iter().collect();
    let mut issues = Vec::new();

    fn walk_for_regex_in_loop(
        stmts: &[Stmt],
        loop_depth: usize,
        module: &ModuleIndex,
        re_funcs: &HashSet<&str>,
        issues: &mut Vec<Issue>,
    ) {
        for stmt in stmts {
            match stmt {
                Stmt::For(node) => {
                    walk_for_regex_in_loop(&node.body, loop_depth + 1, module, re_funcs, issues);
                    walk_for_regex_in_loop(&node.orelse, loop_depth, module, re_funcs, issues);
                }
                Stmt::AsyncFor(node) => {
                    walk_for_regex_in_loop(&node.body, loop_depth + 1, module, re_funcs, issues);
                    walk_for_regex_in_loop(&node.orelse, loop_depth, module, re_funcs, issues);
                }
                Stmt::While(node) => {
                    walk_for_regex_in_loop(&node.body, loop_depth + 1, module, re_funcs, issues);
                    walk_for_regex_in_loop(&node.orelse, loop_depth, module, re_funcs, issues);
                }
                Stmt::If(node) => {
                    walk_for_regex_in_loop(&node.body, loop_depth, module, re_funcs, issues);
                    walk_for_regex_in_loop(&node.orelse, loop_depth, module, re_funcs, issues);
                }
                Stmt::With(node) => {
                    walk_for_regex_in_loop(&node.body, loop_depth, module, re_funcs, issues);
                }
                Stmt::AsyncWith(node) => {
                    walk_for_regex_in_loop(&node.body, loop_depth, module, re_funcs, issues);
                }
                Stmt::Try(node) => {
                    walk_for_regex_in_loop(&node.body, loop_depth, module, re_funcs, issues);
                    for handler in &node.handlers {
                        let ast::ExceptHandler::ExceptHandler(handler) = handler;
                        walk_for_regex_in_loop(&handler.body, loop_depth, module, re_funcs, issues);
                    }
                    walk_for_regex_in_loop(&node.orelse, loop_depth, module, re_funcs, issues);
                    walk_for_regex_in_loop(&node.finalbody, loop_depth, module, re_funcs, issues);
                }
                Stmt::FunctionDef(node) => {
                    walk_for_regex_in_loop(&node.body, 0, module, re_funcs, issues);
                }
                Stmt::AsyncFunctionDef(node) => {
                    walk_for_regex_in_loop(&node.body, 0, module, re_funcs, issues);
                }
                Stmt::ClassDef(node) => {
                    walk_for_regex_in_loop(&node.body, 0, module, re_funcs, issues);
                }
                _ => {}
            }
            if loop_depth > 0 {
                // Check expressions in this statement for re.* calls
                walk_stmt_exprs(stmt, &mut |expr| {
                    let Expr::Call(call) = expr else { return };
                    if let Expr::Attribute(func) = &*call.func {
                        if let Expr::Name(base) = &*func.value {
                            if base.id.as_str() == "re" && re_funcs.contains(func.attr.as_str()) {
                                if call.args.first().is_some_and(|a| matches!(a, Expr::Constant(c) if matches!(c.value, ast::Constant::Str(_)))) {
                                    let line = module.line_for_offset(call.range.start().to_usize());
                                    if !module.is_rule_suppressed(line, "performance/regex-in-loop") {
                                        issues.push(Issue {
                                            check: "performance/regex-in-loop",
                                            severity: "warning",
                                            category: "Performance",
                                            line,
                                            path: module.rel_path.to_string(),
                                            message: Box::leak(
                                                format!("re.{}() with literal pattern inside loop — hoist to module level", func.attr).into_boxed_str(),
                                            ),
                                            help: "Compile regex patterns outside loops: PATTERN = re.compile('...') at module level.",
                                        });
                                    }
                                }
                            }
                        }
                    }
                });
            }
        }
    }

    walk_for_regex_in_loop(suite, 0, module, &re_funcs, &mut issues);
    issues
}

// ── Performance: n-plus-one-hint ────────────────────────────────────────

pub(crate) fn collect_n_plus_one_hint_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
    let lower_source = module.source.to_ascii_lowercase();
    if !["session", "db", "database", "conn", "connection", "cursor"]
        .iter()
        .any(|h| lower_source.contains(h))
    {
        return Vec::new();
    }
    let db_attrs: HashSet<&str> = ["query", "execute", "get", "filter", "filter_by", "all", "first", "one", "scalars", "scalar"].into_iter().collect();
    let session_hints: HashSet<&str> = ["session", "db", "database", "conn", "connection", "cursor"].into_iter().collect();
    let mut issues = Vec::new();
    let mut seen_lines: HashSet<usize> = HashSet::new();

    fn walk_for_db_in_loop(
        stmts: &[Stmt],
        loop_names: &HashSet<String>,
        in_loop: bool,
        module: &ModuleIndex,
        db_attrs: &HashSet<&str>,
        session_hints: &HashSet<&str>,
        issues: &mut Vec<Issue>,
        seen_lines: &mut HashSet<usize>,
    ) {
        for stmt in stmts {
            match stmt {
                Stmt::For(node) => {
                    let mut new_names = loop_names.clone();
                    walk_expr_tree(&node.target, &mut |expr| {
                        if let Expr::Name(n) = expr {
                            new_names.insert(n.id.to_string());
                        }
                    });
                    walk_for_db_in_loop(&node.body, &new_names, true, module, db_attrs, session_hints, issues, seen_lines);
                    walk_for_db_in_loop(&node.orelse, loop_names, in_loop, module, db_attrs, session_hints, issues, seen_lines);
                }
                Stmt::While(node) => {
                    let mut new_names = loop_names.clone();
                    walk_expr_tree(&node.test, &mut |expr| {
                        if let Expr::Name(n) = expr {
                            new_names.insert(n.id.to_string());
                        }
                    });
                    walk_for_db_in_loop(&node.body, &new_names, true, module, db_attrs, session_hints, issues, seen_lines);
                    walk_for_db_in_loop(&node.orelse, loop_names, in_loop, module, db_attrs, session_hints, issues, seen_lines);
                }
                Stmt::FunctionDef(node) => {
                    walk_for_db_in_loop(&node.body, &HashSet::new(), false, module, db_attrs, session_hints, issues, seen_lines);
                }
                Stmt::AsyncFunctionDef(node) => {
                    walk_for_db_in_loop(&node.body, &HashSet::new(), false, module, db_attrs, session_hints, issues, seen_lines);
                }
                Stmt::ClassDef(node) => {
                    walk_for_db_in_loop(&node.body, &HashSet::new(), false, module, db_attrs, session_hints, issues, seen_lines);
                }
                Stmt::If(node) => {
                    walk_for_db_in_loop(&node.body, loop_names, in_loop, module, db_attrs, session_hints, issues, seen_lines);
                    walk_for_db_in_loop(&node.orelse, loop_names, in_loop, module, db_attrs, session_hints, issues, seen_lines);
                }
                Stmt::With(node) => {
                    walk_for_db_in_loop(&node.body, loop_names, in_loop, module, db_attrs, session_hints, issues, seen_lines);
                }
                Stmt::Try(node) => {
                    walk_for_db_in_loop(&node.body, loop_names, in_loop, module, db_attrs, session_hints, issues, seen_lines);
                    for handler in &node.handlers {
                        let ast::ExceptHandler::ExceptHandler(handler) = handler;
                        walk_for_db_in_loop(&handler.body, loop_names, in_loop, module, db_attrs, session_hints, issues, seen_lines);
                    }
                }
                _ => {}
            }
            if in_loop && !loop_names.is_empty() {
                walk_stmt_exprs(stmt, &mut |expr| {
                    let Expr::Call(call) = expr else { return };
                    if let Expr::Attribute(func) = &*call.func {
                        if !db_attrs.contains(func.attr.as_str()) {
                            return;
                        }
                        if let Expr::Name(obj) = &*func.value {
                            if !session_hints.contains(obj.id.to_ascii_lowercase().as_str()) {
                                return;
                            }
                            // Check if loop variable is referenced in the call
                            let mut refs_loop = false;
                            walk_expr_tree(expr, &mut |inner| {
                                if let Expr::Name(n) = inner {
                                    if loop_names.contains(n.id.as_str()) {
                                        refs_loop = true;
                                    }
                                }
                            });
                            if refs_loop {
                                let line = module.line_for_offset(call.range.start().to_usize());
                                if !seen_lines.contains(&line) && !module.is_rule_suppressed(line, "performance/n-plus-one-hint") {
                                    seen_lines.insert(line);
                                    issues.push(Issue {
                                        check: "performance/n-plus-one-hint",
                                        severity: "warning",
                                        category: "Performance",
                                        line,
                                        path: module.rel_path.to_string(),
                                        message: Box::leak(
                                            format!("Potential N+1: {}.{}() inside loop — batch with IN clause or join", obj.id, func.attr).into_boxed_str(),
                                        ),
                                        help: "Collect IDs first, then query in batch: session.query(M).filter(M.id.in_(ids))",
                                    });
                                }
                            }
                        }
                    }
                });
            }
        }
    }

    walk_for_db_in_loop(suite, &HashSet::new(), false, module, &db_attrs, &session_hints, &mut issues, &mut seen_lines);
    issues
}

// ── Correctness: get-with-side-effect ───────────────────────────────────

pub(crate) fn collect_sequential_awaits_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
    if !module.source.contains("await ") {
        return Vec::new();
    }
    let side_effect_attrs: HashSet<&str> = [
        "commit", "rollback", "flush", "close", "aclose", "emit",
        "publish", "send", "save", "delete", "create", "update", "insert",
    ].into_iter().collect();
    let session_hints: HashSet<&str> = [
        "session", "db", "database", "conn", "connection", "cursor",
        "async_session", "db_session",
    ].into_iter().collect();

    let mut issues = Vec::new();

    fn is_await_assign(stmt: &Stmt) -> Option<(&Expr, HashSet<String>)> {
        let (targets, value) = match stmt {
            Stmt::Assign(node) => (node.targets.as_slice(), &*node.value),
            Stmt::AnnAssign(node) => {
                if let Some(val) = &node.value {
                    (std::slice::from_ref(&*node.target), &**val)
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        let Expr::Await(await_node) = value else { return None };
        let Expr::Call(call) = &*await_node.value else { return None };
        let mut names = HashSet::new();
        for target in targets {
            walk_expr_tree(target, &mut |expr| {
                if let Expr::Name(n) = expr {
                    names.insert(n.id.to_string());
                }
            });
        }
        Some((&*call.func, names))
    }

    fn shared_session_name(call_func: &Expr, session_hints: &HashSet<&str>) -> Option<String> {
        if let Expr::Attribute(a) = call_func {
            if let Expr::Name(base) = &*a.value {
                if session_hints.contains(base.id.to_ascii_lowercase().as_str()) {
                    return Some(base.id.to_string());
                }
            }
        }
        None
    }

    for function in all_functions(suite) {
        if !function.is_async {
            continue;
        }
        let body = function.body;
        let mut i = 0;
        while i < body.len().saturating_sub(1) {
            let run_start = i;
            let mut run_count = 0;
            let mut assigned_so_far: HashSet<String> = HashSet::new();
            let mut run_sessions: Vec<Option<String>> = Vec::new();
            let _run_parallelisable = true;

            while i < body.len() {
                let Some((call_func, assigned_names)) = is_await_assign(&body[i]) else { break };

                // Check side-effect calls
                let is_side_effect = match call_func {
                    Expr::Attribute(a) => side_effect_attrs.contains(a.attr.as_str()),
                    Expr::Name(n) => n.id.starts_with("emit_") || n.id.starts_with("log_") || n.id.starts_with("save_"),
                    _ => false,
                };
                if is_side_effect {
                    break;
                }

                // Check data dependencies
                let mut used_names: HashSet<String> = HashSet::new();
                walk_expr_tree(call_func, &mut |expr| {
                    if let Expr::Name(n) = expr {
                        used_names.insert(n.id.to_string());
                    }
                });
                if !used_names.is_disjoint(&assigned_so_far) {
                    break;
                }

                run_sessions.push(shared_session_name(call_func, &session_hints));
                assigned_so_far.extend(assigned_names);
                run_count += 1;
                i += 1;
            }

            if run_count >= 2 {
                // Skip if all share same DB session
                let all_same_session = run_sessions.first().is_some_and(|first| {
                    first.is_some() && run_sessions.iter().all(|s| s == first)
                });
                if !all_same_session {
                    let line = module.line_for_offset(body[run_start].range().start().to_usize());
                    if !module.is_rule_suppressed(line, "performance/sequential-awaits") {
                        issues.push(Issue {
                            check: "performance/sequential-awaits",
                            severity: "warning",
                            category: "Performance",
                            line,
                            path: module.rel_path.to_string(),
                            message: Box::leak(
                                format!(
                                    "{} sequential awaits in {}() could use asyncio.gather()",
                                    run_count, function.name
                                )
                                .into_boxed_str(),
                            ),
                            help: "Independent awaits can run concurrently: results = await asyncio.gather(coro1(), coro2())",
                        });
                    }
                }
            }
            i = i.max(run_start + 1);
        }
    }
    issues
}
