use rustpython_parser::ast::{self, Expr, Pattern, Ranged, Stmt};
use std::collections::HashSet;

use fastapi_doctor_core::ast_helpers::*;
use fastapi_doctor_core::{Config, ImportSurfaceSummary, Issue, ModuleIndex};

use crate::engine::RuleSelection;

pub(crate) fn collect_async_without_await_issues(
    module: &ModuleIndex,
    function_index: &FunctionIndex,
) -> Vec<Issue> {
    let mut unnecessary_async = HashSet::new();

    for function in &function_index.functions {
        if function.is_async
            && !function.has_async_constructs
            && !function.is_stub_body
            && !function.is_abstractmethod
            && !function.owner_is_protocol
        {
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
    function_index: &FunctionIndex,
    rules: &RuleSelection,
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

        let is_route_handler = function_index
            .functions
            .iter()
            .find(|candidate| candidate.name == function.name && candidate.line == line)
            .is_some_and(|candidate| candidate.is_route_handler);

        if rules.giant_route_handler
            && is_route_handler
            && config.giant_function_threshold > 0
            && size > config.giant_function_threshold
        {
            issues.push(Issue {
                check: "architecture/giant-route-handler",
                severity: "error",
                category: "Architecture",
                line,
                path: module.rel_path.to_string(),
                message: Box::leak(
                    format!(
                        "Route handler '{}' is {} lines (>{}) — split request handling from business logic",
                        function.name, size, config.giant_function_threshold
                    )
                    .into_boxed_str(),
                ),
                help: "Large API handlers hide validation, auth, and side-effect boundaries. Move orchestration to services and keep request handlers narrow.",
            });
        } else if rules.giant_function
            && config.giant_function_threshold > 0
            && size > config.giant_function_threshold
        {
            issues.push(Issue {
                check: "architecture/giant-function",
                severity: "warning",
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
        } else if rules.large_function
            && config.large_function_threshold > 0
            && size > config.large_function_threshold
        {
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

pub(crate) fn collect_import_bloat_issue(
    module: &ModuleIndex,
    summary: &ImportSurfaceSummary,
    threshold: usize,
) -> Option<Issue> {
    if threshold == 0 || summary.score <= threshold {
        return None;
    }

    let dependency_preview = summary
        .dependencies
        .iter()
        .take(3)
        .map(|dependency| {
            let preview = dependency
                .symbol_paths
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}[{preview}]", dependency.dependency)
        })
        .collect::<Vec<_>>()
        .join("; ");

    let extraction_candidates = summary
        .dependencies
        .iter()
        .filter(|dependency| dependency.symbol_paths.len() <= 4)
        .take(3)
        .map(|dependency| {
            format!(
                "{}({})",
                dependency.dependency,
                dependency.symbol_paths.len()
            )
        })
        .collect::<Vec<_>>()
        .join(", ");

    let mut message = format!(
        "File touches {} imported symbols across {} dependencies (>{})",
        summary.score, summary.dependency_count, threshold
    );
    if !dependency_preview.is_empty() {
        message.push_str(&format!(" — {}", dependency_preview));
    }

    let mut help = "Count actual imported symbol usage, not raw import lines. Split low-surface dependencies into adapters, move type-only imports behind TYPE_CHECKING, and lazy-import bulky libraries at the boundary.".to_string();
    if !extraction_candidates.is_empty() {
        help.push_str(&format!(
            " Small-surface extraction candidates: {extraction_candidates}."
        ));
    }

    Some(Issue {
        check: "architecture/import-bloat",
        severity: "warning",
        category: "Architecture",
        line: 0,
        path: module.rel_path.to_string(),
        message: Box::leak(message.into_boxed_str()),
        help: Box::leak(help.into_boxed_str()),
    })
}

// =========================================================================
// New check implementations
// =========================================================================

// ── Resilience rules ────────────────────────────────────────────────────

pub(crate) fn collect_avoid_sys_exit_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if module.file_name.as_deref() == Some("__main__.py")
        || module.file_name.as_deref() == Some("cli.py")
        || module.rel_path.contains("scripts/")
    {
        return Vec::new();
    }
    if !module.source.contains("sys.exit")
        && !module.source.contains("exit(")
        && !module.source.contains("quit(")
    {
        return Vec::new();
    }
    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        let is_exit = match &*call.func {
            Expr::Attribute(a) => {
                matches!(&*a.value, Expr::Name(n) if n.id.as_str() == "sys")
                    && a.attr.as_str() == "exit"
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

// ── Correctness: serverless-filesystem-write ────────────────────────────

pub(crate) fn collect_print_in_production_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if module.has_path_part(&["scripts", "lib"]) || !module.source.contains("print(") {
        return Vec::new();
    }

    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        let Expr::Name(func) = &*call.func else {
            return;
        };
        if func.id.as_str() != "print" {
            return;
        }
        let line = module.line_for_offset(call.range.start().to_usize());
        if module.is_rule_suppressed(line, "architecture/print-in-production") {
            return;
        }
        issues.push(Issue {
            check: "architecture/print-in-production",
            severity: "warning",
            category: "Architecture",
            line,
            path: module.rel_path.to_string(),
            message: "print() in production code — use logger instead",
            help: "Replace with logger.info/debug/warning as appropriate.",
        });
    });
    issues
}

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
        let func_line = module.line_for_offset(function.range.start().to_usize());
        if module.is_rule_suppressed_near(func_line, "architecture/fat-route-handler", 8) {
            continue;
        }

        let route_decorator = if func_line >= 2 {
            let dec_start = func_line.saturating_sub(5);
            (dec_start..func_line).find_map(|l| {
                if l > 0 && l <= module.lines.len() {
                    let trimmed = &module.lines[l - 1].trimmed;
                    if trimmed.starts_with("@")
                        && (trimmed.contains("router") || trimmed.contains("app"))
                    {
                        Some(trimmed.as_ref())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
        } else {
            None
        };
        let Some(route_decorator) = route_decorator else {
            continue;
        };
        let end_line = module.line_for_offset(function.range.end().to_usize().saturating_sub(1));
        let func_len = end_line.saturating_sub(func_line) + 1;
        let effective_threshold = if is_mutating_route_decorator(route_decorator) {
            config.fat_route_handler_threshold.saturating_mul(3) / 2
        } else {
            config.fat_route_handler_threshold
        };
        if func_len > effective_threshold {
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
                        "Keep handlers under {} lines ({} for mutating endpoints). Move logic to a service function.",
                        config.fat_route_handler_threshold,
                        config.fat_route_handler_threshold.saturating_mul(3) / 2
                    )
                    .into_boxed_str(),
                ),
            });
        }
    }
    issues
}

fn is_mutating_route_decorator(decorator: &str) -> bool {
    [".post(", ".put(", ".patch(", ".delete("]
        .iter()
        .any(|marker| decorator.contains(marker))
}

// ── Architecture: passthrough-function ──────────────────────────────────

pub(crate) fn collect_passthrough_function_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
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
        let Stmt::Return(ret) = &body[0] else {
            continue;
        };
        let Some(ret_val) = &ret.value else { continue };
        let Expr::Call(call) = &**ret_val else {
            continue;
        };

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

const HIDDEN_DEP_CONSTRUCTORS: &[&str] = &[
    "Session",
    "AsyncSession",
    "AsyncClient",
    "Client",
    "KafkaProducer",
    "KafkaConsumer",
    "Redis",
    "StrictRedis",
    "MongoClient",
    "Elasticsearch",
];

const HIDDEN_DEP_PROVIDERS: &[&str] = &[
    "get_db",
    "get_session",
    "get_async_session",
    "get_client",
    "get_http_client",
    "get_settings",
];

pub(crate) fn collect_hidden_dependency_instantiation_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if module.rel_path.contains("tests/")
        || module.rel_path.contains("scripts/")
        || !module.has_path_part(&["routers", "services", "interfaces"])
    {
        return Vec::new();
    }

    let mut issues = Vec::new();
    collect_hidden_dependency_issues_in_block(module, suite, &mut issues);
    issues
}

fn collect_hidden_dependency_issues_in_block(
    module: &ModuleIndex,
    stmts: &[Stmt],
    issues: &mut Vec<Issue>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::FunctionDef(node) => {
                analyze_hidden_dependency_function(
                    module,
                    node.name.as_str(),
                    &node.body,
                    node.range(),
                    issues,
                );
                collect_hidden_dependency_issues_in_block(module, &node.body, issues);
            }
            Stmt::AsyncFunctionDef(node) => {
                analyze_hidden_dependency_function(
                    module,
                    node.name.as_str(),
                    &node.body,
                    node.range(),
                    issues,
                );
                collect_hidden_dependency_issues_in_block(module, &node.body, issues);
            }
            Stmt::ClassDef(node) => {
                collect_hidden_dependency_issues_in_block(module, &node.body, issues);
            }
            _ => {}
        }
    }
}

fn analyze_hidden_dependency_function(
    module: &ModuleIndex,
    name: &str,
    body: &[Stmt],
    range: rustpython_parser::ast::text_size::TextRange,
    issues: &mut Vec<Issue>,
) {
    if name == "__init__" {
        return;
    }

    let source = module.source_slice(range);
    if source.contains("# noqa: architecture") {
        return;
    }

    let nested_ranges = nested_def_ranges(body);
    let mut first_hit: Option<(usize, &'static str)> = None;

    walk_suite_exprs(body, &mut |expr| {
        if first_hit.is_some() || expr_inside_nested_range(expr.range(), &nested_ranges) {
            return;
        }

        let Expr::Call(call) = expr else {
            return;
        };
        if let Some(dep_name) = hidden_dependency_call_name(call) {
            let line = module.line_for_offset(call.range.start().to_usize());
            if !module.is_rule_suppressed(line, "architecture/hidden-dependency-instantiation") {
                first_hit = Some((line, dep_name));
            }
        }
    });

    let Some((line, dep_name)) = first_hit else {
        return;
    };
    issues.push(Issue {
        check: "architecture/hidden-dependency-instantiation",
        severity: "warning",
        category: "Architecture",
        line,
        path: module.rel_path.to_string(),
        message: Box::leak(
            format!(
                "Function '{}' resolves dependency '{}' inside its body",
                name, dep_name
            )
            .into_boxed_str(),
        ),
        help: "Inject dependencies via function arguments, Depends(...), or class __init__ instead of wiring them inside application logic. Lazy imports for performance are fine; hidden dependency resolution is not.",
    });
}

fn nested_def_ranges(stmts: &[Stmt]) -> Vec<rustpython_parser::ast::text_size::TextRange> {
    let mut ranges = Vec::new();
    for stmt in stmts {
        match stmt {
            Stmt::FunctionDef(node) => ranges.push(node.range()),
            Stmt::AsyncFunctionDef(node) => ranges.push(node.range()),
            Stmt::ClassDef(node) => ranges.push(node.range()),
            _ => {}
        }
    }
    ranges
}

fn expr_inside_nested_range(
    range: rustpython_parser::ast::text_size::TextRange,
    nested_ranges: &[rustpython_parser::ast::text_size::TextRange],
) -> bool {
    let start = range.start().to_usize();
    nested_ranges.iter().any(|nested| {
        let nested_start = nested.start().to_usize();
        let nested_end = nested.end().to_usize();
        nested_start <= start && start < nested_end
    })
}

fn hidden_dependency_call_name(call: &ast::ExprCall) -> Option<&'static str> {
    match &*call.func {
        Expr::Name(name) => hidden_dependency_name(name.id.as_str(), true),
        Expr::Attribute(attr) => {
            let is_self_like = matches!(
                &*attr.value,
                Expr::Name(base) if matches!(base.id.as_str(), "self" | "cls" | "super")
            );
            if is_self_like {
                None
            } else {
                hidden_dependency_name(attr.attr.as_str(), false)
            }
        }
        _ => None,
    }
}

fn hidden_dependency_name(name: &str, allow_provider_functions: bool) -> Option<&'static str> {
    HIDDEN_DEP_CONSTRUCTORS
        .iter()
        .copied()
        .find(|candidate| *candidate == name)
        .or_else(|| {
            allow_provider_functions.then_some(()).and_then(|_| {
                HIDDEN_DEP_PROVIDERS
                    .iter()
                    .copied()
                    .find(|candidate| *candidate == name)
            })
        })
}

pub(crate) fn collect_flag_argument_dispatch_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    let mut issues = Vec::new();

    for stmt in suite {
        let (args, body, name, range) = match stmt {
            Stmt::FunctionDef(node) => (&node.args, &node.body, node.name.as_str(), node.range()),
            Stmt::AsyncFunctionDef(node) => {
                (&node.args, &node.body, node.name.as_str(), node.range())
            }
            _ => continue,
        };

        let param_names: HashSet<&str> = args
            .args
            .iter()
            .skip(usize::from(
                args.args
                    .first()
                    .is_some_and(|arg| arg.def.arg.as_str() == "self"),
            ))
            .map(|arg| arg.def.arg.as_str())
            .collect();
        if param_names.is_empty() {
            continue;
        }

        let source = module.source_slice(range);
        if source.contains("# noqa: architecture") {
            continue;
        }

        let Some(discriminant) = top_level_flag_discriminant(body, &param_names) else {
            continue;
        };
        let line = module.line_for_offset(range.start().to_usize());
        if module.is_rule_suppressed(line, "architecture/flag-argument-dispatch") {
            continue;
        }

        issues.push(Issue {
            check: "architecture/flag-argument-dispatch",
            severity: "warning",
            category: "Architecture",
            line,
            path: module.rel_path.to_string(),
            message: Box::leak(
                format!(
                    "Function '{}' dispatches behavior by branching on parameter '{}'",
                    name, discriminant
                )
                .into_boxed_str(),
            ),
            help: "Split target-specific behavior into explicit functions or strategies instead of branching on a mode/target flag.",
        });
    }

    issues
}

fn top_level_flag_discriminant<'a>(body: &'a [Stmt], params: &HashSet<&'a str>) -> Option<&'a str> {
    for stmt in body {
        match stmt {
            Stmt::If(node) => {
                if !node.orelse.is_empty()
                    && branch_body_has_material_behavior(&node.body)
                    && branch_body_has_material_behavior(&node.orelse)
                {
                    if let Some(name) = compare_uses_param_literal(&node.test, params) {
                        return Some(name);
                    }
                }
            }
            Stmt::Match(node) => {
                if let Expr::Name(subject) = &*node.subject {
                    if params.contains(subject.id.as_str())
                        && node.cases.len() >= 2
                        && node.cases.iter().all(|case| {
                            case.guard.is_none()
                                && pattern_is_literal_like(&case.pattern)
                                && branch_body_has_material_behavior(&case.body)
                        })
                    {
                        return Some(subject.id.as_str());
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn compare_uses_param_literal<'a>(expr: &'a Expr, params: &HashSet<&'a str>) -> Option<&'a str> {
    let Expr::Compare(compare) = expr else {
        return None;
    };
    if compare.ops.len() != 1 || compare.comparators.len() != 1 {
        return None;
    }
    let left_name = match &*compare.left {
        Expr::Name(name) if params.contains(name.id.as_str()) => Some(name.id.as_str()),
        _ => None,
    };
    let right_name = match &compare.comparators[0] {
        Expr::Name(name) if params.contains(name.id.as_str()) => Some(name.id.as_str()),
        _ => None,
    };
    if let Some(name) = left_name {
        if expr_is_literal_like(&compare.comparators[0]) {
            return Some(name);
        }
    }
    if let Some(name) = right_name {
        if expr_is_literal_like(&compare.left) {
            return Some(name);
        }
    }
    None
}

fn expr_is_literal_like(expr: &Expr) -> bool {
    matches!(expr, Expr::Constant(_))
        || matches!(expr, Expr::Attribute(_))
        || matches!(expr, Expr::Name(name) if matches!(name.id.as_str(), "True" | "False"))
}

fn pattern_is_literal_like(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::MatchValue(node) => expr_is_literal_like(&node.value),
        Pattern::MatchSingleton(_) => true,
        Pattern::MatchOr(node) => node.patterns.iter().all(pattern_is_literal_like),
        _ => false,
    }
}

fn branch_body_has_material_behavior(body: &[Stmt]) -> bool {
    body.iter().any(|stmt| match stmt {
        Stmt::Expr(expr) => matches!(&*expr.value, Expr::Call(_)),
        Stmt::Assign(_) | Stmt::AnnAssign(_) | Stmt::AugAssign(_) => true,
        Stmt::Return(ret) => ret.value.is_some(),
        Stmt::Raise(_) => true,
        _ => false,
    })
}
