use std::collections::HashMap;
use std::path::Path;

use crate::ast_helpers;
use rustpython_parser::ast::{self, Constant, Expr, Ranged, Stmt};
use rustpython_parser::Parse;

#[derive(Clone, Debug)]
pub struct ModuleRecord {
    pub rel_path: String,
    pub source: String,
}

#[derive(Clone, Default)]
pub struct Config {
    pub import_bloat_threshold: usize,
    pub giant_function_threshold: usize,
    pub large_function_threshold: usize,
    pub deep_nesting_threshold: usize,
    pub god_module_threshold: usize,
    pub fat_route_handler_threshold: usize,
    pub should_be_model_mode: String,
    pub forbidden_write_params: Vec<String>,
    pub create_post_prefixes: Vec<String>,
    pub tag_required_prefixes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub check: &'static str,
    pub severity: &'static str,
    pub category: &'static str,
    pub line: usize,
    pub path: String,
    pub message: &'static str,
    pub help: &'static str,
}

#[derive(Clone, Default)]
struct RouterMeta {
    prefix: String,
    tags: Vec<String>,
}

#[derive(Clone)]
pub struct RouteDraft {
    pub router_name: Option<String>,
    pub path: String,
    pub methods: Vec<String>,
    pub dependency_names: Vec<String>,
    pub param_names: Vec<String>,
    pub include_in_schema: bool,
    pub has_response_model: bool,
    pub response_model_str: Option<String>,
    pub status_code: Option<usize>,
    pub decorator_tags: Vec<String>,
    pub endpoint_name: String,
    pub has_docstring: bool,
    pub source_file: String,
    pub line: usize,
    pub local_prefix: String,
    pub local_tags: Vec<String>,
}

#[derive(Clone)]
pub struct SuppressionRecord {
    pub rule: String,
    pub reason: String,
    pub path: String,
    pub line: usize,
}

#[derive(Clone, Debug)]
pub struct RouteRecord {
    pub path: String,
    pub methods: Vec<String>,
    pub dependency_names: Vec<String>,
    pub param_names: Vec<String>,
    pub include_in_schema: bool,
    pub has_response_model: bool,
    pub response_model_str: Option<String>,
    pub status_code: Option<usize>,
    pub tags: Vec<String>,
    pub endpoint_name: String,
    pub has_docstring: bool,
    pub source_file: String,
    pub line: usize,
}

pub type IssueTuple = (String, String, String, usize, String, String, String);
pub type RouteTuple = (
    String,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    bool,
    bool,
    Option<String>,
    Option<usize>,
    Vec<String>,
    String,
    bool,
    String,
);
pub type SuppressionTuple = (String, String, String, usize);

#[derive(Clone, Default)]
pub struct ScoreSummary {
    pub categories: HashMap<String, usize>,
    pub score: usize,
    pub label: String,
}

#[derive(Clone, Default)]
pub struct RouteScan {
    pub drafts: Vec<RouteDraft>,
    pub includes: Vec<(String, String, Vec<String>)>,
}

pub struct LineRecord<'a> {
    pub number: usize,
    pub raw: &'a str,
    pub trimmed: &'a str,
    pub trimmed_start: &'a str,
    pub compact: String,
}

pub struct ModuleIndex<'a> {
    pub rel_path: &'a str,
    pub source: &'a str,
    pub lines: Vec<LineRecord<'a>>,
    pub line_starts: Vec<usize>,
    pub path_parts: Vec<String>,
    pub file_name: Option<String>,
    pub import_count: usize,
    pub has_noqa_architecture: bool,
}

impl<'a> ModuleIndex<'a> {
    pub fn new(module: &'a ModuleRecord) -> Self {
        let path = Path::new(&module.rel_path);
        let path_parts: Vec<String> = path
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect();
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());

        let mut line_starts = vec![0];
        let mut lines = Vec::new();
        let mut import_count = 0;
        for (idx, raw) in module.source.lines().enumerate() {
            let trimmed_start = raw.trim_start();
            if trimmed_start.starts_with("import ") || trimmed_start.starts_with("from ") {
                import_count += 1;
            }
            lines.push(LineRecord {
                number: idx + 1,
                raw,
                trimmed: raw.trim(),
                trimmed_start,
                compact: normalized_no_space(raw),
            });
        }
        for (idx, byte) in module.source.as_bytes().iter().enumerate() {
            if *byte == b'\n' {
                line_starts.push(idx + 1);
            }
        }

        Self {
            rel_path: &module.rel_path,
            source: &module.source,
            lines,
            line_starts,
            path_parts,
            file_name,
            import_count,
            has_noqa_architecture: module.source.contains("# noqa: architecture"),
        }
    }

    pub fn has_path_part(&self, expected: &[&str]) -> bool {
        self.path_parts
            .iter()
            .any(|part| expected.iter().any(|candidate| part == candidate))
    }

    pub fn line_for_offset(&self, offset: usize) -> usize {
        self.line_starts.partition_point(|start| *start <= offset)
    }

    pub fn source_slice(&self, range: rustpython_parser::ast::text_size::TextRange) -> &str {
        let start = range.start().to_usize();
        let end = range.end().to_usize();
        self.source.get(start..end).unwrap_or("")
    }

    pub fn is_rule_suppressed(&self, line_number: usize, rule_id: &str) -> bool {
        if line_number == 0 || line_number > self.lines.len() {
            return false;
        }
        line_suppresses_rule(&self.lines[line_number - 1].raw, rule_id)
    }
}

pub fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn parse_bool_expr(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::Constant(node) => match &node.value {
            Constant::Bool(value) => Some(*value),
            _ => None,
        },
        _ => None,
    }
}

fn parse_string_expr(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Constant(node) => match &node.value {
            Constant::Str(value) => Some(value.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn parse_string_list(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::List(node) => node.elts.iter().filter_map(parse_string_expr).collect(),
        Expr::Tuple(node) => node.elts.iter().filter_map(parse_string_expr).collect(),
        _ => Vec::new(),
    }
}

fn parse_usize_expr(module: &ModuleIndex<'_>, expr: &Expr) -> Option<usize> {
    module
        .source_slice(expr.range())
        .trim()
        .parse::<usize>()
        .ok()
}

fn keyword_value<'a>(keywords: &'a [ast::Keyword], name: &str) -> Option<&'a Expr> {
    keywords
        .iter()
        .find(|kw| kw.arg.as_deref() == Some(name))
        .map(|kw| &kw.value)
}

fn call_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(node) => Some(node.id.to_string()),
        Expr::Attribute(node) => Some(node.attr.to_string()),
        _ => None,
    }
}

fn router_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(node) => Some(node.id.to_string()),
        Expr::Attribute(node) => Some(node.attr.to_string()),
        _ => None,
    }
}

fn method_names(call: &ast::ExprCall) -> Option<Vec<String>> {
    let Expr::Attribute(func) = &*call.func else {
        return None;
    };
    let method_name = func.attr.to_ascii_lowercase();
    if [
        "get", "post", "put", "patch", "delete", "head", "options", "trace",
    ]
    .contains(&method_name.as_str())
    {
        let methods = vec![method_name.to_ascii_uppercase()]
            .into_iter()
            .filter(|method| method != "HEAD" && method != "OPTIONS")
            .collect::<Vec<_>>();
        return if methods.is_empty() {
            None
        } else {
            Some(methods)
        };
    }
    if method_name != "api_route" {
        return None;
    }
    let methods = keyword_value(&call.keywords, "methods")
        .map(parse_string_list)
        .unwrap_or_else(|| vec!["GET".to_string()])
        .into_iter()
        .map(|method| method.to_ascii_uppercase())
        .filter(|method| method != "HEAD" && method != "OPTIONS")
        .collect::<Vec<_>>();
    if methods.is_empty() {
        None
    } else {
        Some(methods)
    }
}

fn depends_name(call: &ast::ExprCall) -> Option<String> {
    call.args.first().and_then(|expr| match expr {
        Expr::Name(node) => Some(node.id.to_string()),
        Expr::Attribute(node) => Some(node.attr.to_string()),
        _ => None,
    })
}

fn is_depends_call(expr: &Expr) -> bool {
    match expr {
        Expr::Call(call) => match &*call.func {
            Expr::Name(node) => node.id.as_str() == "Depends",
            Expr::Attribute(node) => node.attr.as_str() == "Depends",
            _ => false,
        },
        _ => false,
    }
}

fn function_dependency_names(args: &ast::Arguments) -> Vec<String> {
    ast_helpers::function_default_exprs(args)
        .into_iter()
        .filter_map(|expr| match expr {
            Expr::Call(call) if is_depends_call(&expr) => depends_name(&call),
            _ => None,
        })
        .collect()
}

fn decorator_dependency_names(call: &ast::ExprCall) -> Vec<String> {
    let Some(expr) = keyword_value(&call.keywords, "dependencies") else {
        return Vec::new();
    };
    match expr {
        Expr::List(node) => node
            .elts
            .iter()
            .filter_map(|elt| match elt {
                Expr::Call(call) if is_depends_call(elt) => depends_name(call),
                _ => None,
            })
            .collect(),
        Expr::Tuple(node) => node
            .elts
            .iter()
            .filter_map(|elt| match elt {
                Expr::Call(call) if is_depends_call(elt) => depends_name(call),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn function_param_names(args: &ast::Arguments) -> Vec<String> {
    args.posonlyargs
        .iter()
        .chain(args.args.iter())
        .map(|arg| arg.def.arg.to_string())
        .chain(args.kwonlyargs.iter().map(|arg| arg.def.arg.to_string()))
        .filter(|arg| arg != "self")
        .collect()
}

fn has_docstring(body: &[Stmt]) -> bool {
    body.first().is_some_and(|stmt| {
        matches!(stmt, Stmt::Expr(expr) if matches!(&*expr.value, Expr::Constant(node) if matches!(node.value, Constant::Str(_))))
    })
}

fn collect_route_scan_stmt(
    module: &ModuleIndex<'_>,
    stmt: &Stmt,
    routers: &mut HashMap<String, RouterMeta>,
    includes: &mut Vec<(String, String, Vec<String>)>,
    drafts: &mut Vec<RouteDraft>,
) {
    match stmt {
        Stmt::Assign(node) => {
            if let Expr::Call(call) = &*node.value {
                if let Some(name) = call_name(&call.func) {
                    if name == "APIRouter" || name == "FastAPI" {
                        let prefix = keyword_value(&call.keywords, "prefix")
                            .and_then(parse_string_expr)
                            .unwrap_or_default();
                        let tags = keyword_value(&call.keywords, "tags")
                            .map(parse_string_list)
                            .unwrap_or_default();
                        for target in &node.targets {
                            if let Expr::Name(name) = target {
                                routers.insert(
                                    name.id.to_string(),
                                    RouterMeta {
                                        prefix: prefix.clone(),
                                        tags: tags.clone(),
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
        Stmt::Expr(node) => {
            if let Expr::Call(call) = &*node.value {
                if matches!(&*call.func, Expr::Attribute(func) if func.attr.as_str() == "include_router")
                {
                    if let Some(router_expr) = call.args.first() {
                        if let Some(name) = router_name(router_expr) {
                            let prefix = keyword_value(&call.keywords, "prefix")
                                .and_then(parse_string_expr)
                                .unwrap_or_default();
                            let tags = keyword_value(&call.keywords, "tags")
                                .map(parse_string_list)
                                .unwrap_or_default();
                            includes.push((name, prefix, tags));
                        }
                    }
                }
            }
        }
        Stmt::FunctionDef(node) => {
            collect_route_draft(
                module,
                &node.name,
                &node.args,
                &node.decorator_list,
                &node.body,
                node.range(),
                drafts,
            );
            for inner in &node.body {
                collect_route_scan_stmt(module, inner, routers, includes, drafts);
            }
        }
        Stmt::AsyncFunctionDef(node) => {
            collect_route_draft(
                module,
                &node.name,
                &node.args,
                &node.decorator_list,
                &node.body,
                node.range(),
                drafts,
            );
            for inner in &node.body {
                collect_route_scan_stmt(module, inner, routers, includes, drafts);
            }
        }
        Stmt::ClassDef(node) => {
            for inner in &node.body {
                collect_route_scan_stmt(module, inner, routers, includes, drafts);
            }
        }
        Stmt::If(node) => {
            for inner in &node.body {
                collect_route_scan_stmt(module, inner, routers, includes, drafts);
            }
            for inner in &node.orelse {
                collect_route_scan_stmt(module, inner, routers, includes, drafts);
            }
        }
        Stmt::For(node) => {
            for inner in &node.body {
                collect_route_scan_stmt(module, inner, routers, includes, drafts);
            }
            for inner in &node.orelse {
                collect_route_scan_stmt(module, inner, routers, includes, drafts);
            }
        }
        Stmt::AsyncFor(node) => {
            for inner in &node.body {
                collect_route_scan_stmt(module, inner, routers, includes, drafts);
            }
            for inner in &node.orelse {
                collect_route_scan_stmt(module, inner, routers, includes, drafts);
            }
        }
        Stmt::While(node) => {
            for inner in &node.body {
                collect_route_scan_stmt(module, inner, routers, includes, drafts);
            }
            for inner in &node.orelse {
                collect_route_scan_stmt(module, inner, routers, includes, drafts);
            }
        }
        Stmt::With(node) => {
            for inner in &node.body {
                collect_route_scan_stmt(module, inner, routers, includes, drafts);
            }
        }
        Stmt::AsyncWith(node) => {
            for inner in &node.body {
                collect_route_scan_stmt(module, inner, routers, includes, drafts);
            }
        }
        Stmt::Try(node) => {
            for inner in &node.body {
                collect_route_scan_stmt(module, inner, routers, includes, drafts);
            }
            for handler in &node.handlers {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                for inner in &handler.body {
                    collect_route_scan_stmt(module, inner, routers, includes, drafts);
                }
            }
            for inner in &node.orelse {
                collect_route_scan_stmt(module, inner, routers, includes, drafts);
            }
            for inner in &node.finalbody {
                collect_route_scan_stmt(module, inner, routers, includes, drafts);
            }
        }
        Stmt::Match(node) => {
            for case in &node.cases {
                for inner in &case.body {
                    collect_route_scan_stmt(module, inner, routers, includes, drafts);
                }
            }
        }
        _ => {}
    }
}

fn collect_route_draft(
    module: &ModuleIndex<'_>,
    name: &str,
    args: &ast::Arguments,
    decorators: &[Expr],
    body: &[Stmt],
    range: rustpython_parser::ast::text_size::TextRange,
    drafts: &mut Vec<RouteDraft>,
) {
    for decorator in decorators {
        let Expr::Call(call) = decorator else {
            continue;
        };
        let Some(methods) = method_names(call) else {
            continue;
        };
        let router_name = match &*call.func {
            Expr::Attribute(node) => router_name(&node.value),
            _ => None,
        };
        let path = call
            .args
            .first()
            .and_then(|expr| parse_string_expr(expr))
            .unwrap_or_default();
        let include_in_schema = keyword_value(&call.keywords, "include_in_schema")
            .and_then(parse_bool_expr)
            .unwrap_or(true);
        let response_model = keyword_value(&call.keywords, "response_model");
        let status_code = keyword_value(&call.keywords, "status_code")
            .and_then(|expr| parse_usize_expr(module, expr));
        let decorator_tags = keyword_value(&call.keywords, "tags")
            .map(parse_string_list)
            .unwrap_or_default();
        let local_router = router_name
            .as_ref()
            .and_then(|router| Some(router.clone()))
            .unwrap_or_default();
        drafts.push(RouteDraft {
            router_name,
            path,
            methods,
            dependency_names: {
                let mut deps = function_dependency_names(args);
                deps.extend(decorator_dependency_names(call));
                deps
            },
            param_names: function_param_names(args),
            include_in_schema,
            has_response_model: response_model.is_some(),
            response_model_str: response_model
                .map(|expr| module.source_slice(expr.range()).to_ascii_lowercase()),
            status_code,
            decorator_tags,
            endpoint_name: name.to_string(),
            has_docstring: has_docstring(body),
            source_file: module.rel_path.to_string(),
            line: module.line_for_offset(range.start().to_usize()),
            local_prefix: local_router,
            local_tags: Vec::new(),
        });
        return;
    }
}

pub fn extract_route_scan(module: &ModuleIndex<'_>, suite: &ast::Suite) -> RouteScan {
    let mut routers = HashMap::new();
    let mut includes = Vec::new();
    let mut drafts = Vec::new();
    for stmt in suite {
        collect_route_scan_stmt(module, stmt, &mut routers, &mut includes, &mut drafts);
    }
    for draft in &mut drafts {
        if let Some(router_name) = &draft.router_name {
            if let Some(router) = routers.get(router_name) {
                draft.local_prefix = router.prefix.clone();
                draft.local_tags = router.tags.clone();
            } else {
                draft.local_prefix.clear();
                draft.local_tags.clear();
            }
        } else {
            draft.local_prefix.clear();
            draft.local_tags.clear();
        }
    }
    RouteScan { drafts, includes }
}

pub fn finalize_route(
    draft: RouteDraft,
    include_prefix_map: &HashMap<String, (String, Vec<String>)>,
) -> RouteRecord {
    if let Some(router_name) = &draft.router_name {
        if let Some((include_prefix, include_tags)) = include_prefix_map.get(router_name) {
            let full_path = format!("{include_prefix}{}{}", draft.local_prefix, draft.path);
            let tags = if draft.decorator_tags.is_empty() {
                include_tags
                    .iter()
                    .cloned()
                    .chain(draft.local_tags.iter().cloned())
                    .collect()
            } else {
                draft.decorator_tags.clone()
            };
            return RouteRecord {
                path: full_path,
                methods: draft.methods,
                dependency_names: draft.dependency_names,
                param_names: draft.param_names,
                include_in_schema: draft.include_in_schema,
                has_response_model: draft.has_response_model,
                response_model_str: draft.response_model_str,
                status_code: draft.status_code,
                tags,
                endpoint_name: draft.endpoint_name,
                has_docstring: draft.has_docstring,
                source_file: draft.source_file,
                line: draft.line,
            };
        }
    }
    let tags = if draft.decorator_tags.is_empty() {
        draft.local_tags.clone()
    } else {
        draft.decorator_tags.clone()
    };
    RouteRecord {
        path: format!("{}{}", draft.local_prefix, draft.path),
        methods: draft.methods,
        dependency_names: draft.dependency_names,
        param_names: draft.param_names,
        include_in_schema: draft.include_in_schema,
        has_response_model: draft.has_response_model,
        response_model_str: draft.response_model_str,
        status_code: draft.status_code,
        tags,
        endpoint_name: draft.endpoint_name,
        has_docstring: draft.has_docstring,
        source_file: draft.source_file,
        line: draft.line,
    }
}

pub fn route_tuple(route: RouteRecord) -> RouteTuple {
    (
        route.path,
        route.methods,
        route.dependency_names,
        route.param_names,
        route.include_in_schema,
        route.has_response_model,
        route.response_model_str,
        route.status_code,
        route.tags,
        route.endpoint_name,
        route.has_docstring,
        format!("{}:{}", route.source_file, route.line),
    )
}

pub fn collect_suppressions(source: &str, path: &str) -> Vec<SuppressionRecord> {
    source
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let comment = line.split('#').nth(1)?.trim();
            let rest = comment.strip_prefix("doctor:ignore ")?;
            let mut parts = rest.split_whitespace();
            let rule = parts.next()?.to_string();
            let reason = rest
                .split("reason=\"")
                .nth(1)
                .and_then(|tail| tail.split('"').next())
                .unwrap_or("")
                .to_string();
            Some(SuppressionRecord {
                rule,
                reason,
                path: path.to_string(),
                line: index + 1,
            })
        })
        .collect()
}

pub fn issue(
    check: &'static str,
    severity: &'static str,
    category: &'static str,
    line: usize,
    path: &str,
    message: &'static str,
    help: &'static str,
) -> Issue {
    Issue {
        check,
        severity,
        category,
        line,
        path: path.to_string(),
        message,
        help,
    }
}

pub fn normalized_no_space(line: &str) -> String {
    line.chars().filter(|ch| !ch.is_whitespace()).collect()
}

pub fn selector_matches(rule_id: &str, selector: &str) -> bool {
    let selector = selector.trim();
    if selector.ends_with('*') {
        return rule_id.starts_with(&selector[..selector.len().saturating_sub(1)]);
    }
    if selector.ends_with('/') {
        return rule_id.starts_with(selector);
    }
    rule_id == selector
}

pub fn line_suppresses_rule(line: &str, rule_id: &str) -> bool {
    fn normalized_selector(code: &str) -> &str {
        let trimmed = code.trim();
        let end = trimmed
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '*')))
            .unwrap_or(trimmed.len());
        &trimmed[..end]
    }

    for comment in line.split('#').skip(1) {
        let comment = comment.trim();
        if let Some(rest) = comment.strip_prefix("doctor:ignore ") {
            let selector = rest.split_whitespace().next().unwrap_or("");
            if selector_matches(rule_id, selector) {
                return true;
            }
        }

        if let Some(rest) = comment.strip_prefix("noqa") {
            let trimmed = rest.trim();
            if trimmed.is_empty() {
                return true;
            }
            if let Some(codes) = trimmed.strip_prefix(':') {
                for code in codes
                    .split(',')
                    .map(normalized_selector)
                    .filter(|code| !code.is_empty())
                {
                    if selector_matches(rule_id, code) {
                        return true;
                    }
                    let alias = match code {
                        "sql-safe" | "security" => Some("security/"),
                        "architecture" => Some("architecture/"),
                        "correctness" => Some("correctness/"),
                        "performance" => Some("performance/"),
                        "resilience" => Some("resilience/"),
                        "pydantic" => Some("pydantic/"),
                        "config" => Some("config/"),
                        "api-surface" => Some("api-surface/"),
                        "direct-env" => Some("config/direct-env-access"),
                        _ => None,
                    };
                    if alias.is_some_and(|alias_target| selector_matches(rule_id, alias_target)) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub fn parse_suite(module: &ModuleRecord) -> Option<ast::Suite> {
    ast::Suite::parse(&module.source, &module.rel_path).ok()
}

pub fn score_summary(issues: &[Issue]) -> ScoreSummary {
    let mut error_rules = std::collections::HashSet::new();
    let mut warning_rules = std::collections::HashSet::new();
    let mut categories = HashMap::new();

    for issue in issues {
        if issue.severity == "error" {
            error_rules.insert(issue.check);
        } else {
            warning_rules.insert(issue.check);
        }
        *categories.entry(issue.category.to_string()).or_insert(0) += 1;
    }

    let penalty = (error_rules.len() as f64 * 2.0) + warning_rules.len() as f64;
    let score = (100.0 - penalty).round().max(0.0) as usize;
    let label = if score >= 80 {
        "Great"
    } else if score >= 60 {
        "Needs work"
    } else {
        "Critical"
    };

    ScoreSummary {
        categories,
        score,
        label: label.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_summary_perfect_when_no_issues() {
        let summary = score_summary(&[]);
        assert_eq!(summary.score, 100);
        assert_eq!(summary.label, "Great");
        assert!(summary.categories.is_empty());
    }

    #[test]
    fn score_summary_deducts_per_unique_rule() {
        let issues = vec![
            Issue {
                check: "security/unsafe-yaml-load",
                severity: "error",
                category: "Security",
                line: 1,
                path: "a.py".to_string(),
                message: "msg",
                help: "help",
            },
            Issue {
                check: "security/unsafe-yaml-load",
                severity: "error",
                category: "Security",
                line: 5,
                path: "b.py".to_string(),
                message: "msg",
                help: "help",
            },
        ];
        let summary = score_summary(&issues);
        // 2 instances of same rule = 1 unique error rule = -2
        assert_eq!(summary.score, 98);
        assert_eq!(summary.label, "Great");
        assert_eq!(*summary.categories.get("Security").unwrap(), 2);
    }

    #[test]
    fn score_summary_different_severity_weights() {
        let issues = vec![
            Issue {
                check: "security/unsafe-yaml-load",
                severity: "error",
                category: "Security",
                line: 1,
                path: "a.py".to_string(),
                message: "msg",
                help: "help",
            },
            Issue {
                check: "architecture/print-in-production",
                severity: "warning",
                category: "Architecture",
                line: 1,
                path: "a.py".to_string(),
                message: "msg",
                help: "help",
            },
        ];
        let summary = score_summary(&issues);
        // 1 error (-2) + 1 warning (-1) = 97
        assert_eq!(summary.score, 97);
    }

    #[test]
    fn score_summary_floors_at_zero() {
        let mut issues = Vec::new();
        for i in 0..60 {
            issues.push(Issue {
                check: Box::leak(format!("fake/rule-{}", i).into_boxed_str()),
                severity: "error",
                category: "Fake",
                line: 1,
                path: "a.py".to_string(),
                message: "msg",
                help: "help",
            });
        }
        let summary = score_summary(&issues);
        assert_eq!(summary.score, 0);
        assert_eq!(summary.label, "Critical");
    }

    #[test]
    fn score_summary_labels() {
        // Great: >= 80
        let issues = vec![Issue {
            check: "a/b",
            severity: "error",
            category: "X",
            line: 1,
            path: "a.py".to_string(),
            message: "m",
            help: "h",
        }];
        assert_eq!(score_summary(&issues).label, "Great"); // 98

        // Needs work: 60-79
        let mut issues = Vec::new();
        for i in 0..12 {
            issues.push(Issue {
                check: Box::leak(format!("fake/rule-{}", i).into_boxed_str()),
                severity: "error",
                category: "X",
                line: 1,
                path: "a.py".to_string(),
                message: "m",
                help: "h",
            });
        }
        let summary = score_summary(&issues);
        assert_eq!(summary.score, 76); // 100 - 24
        assert_eq!(summary.label, "Needs work");
    }

    #[test]
    fn collect_suppressions_parses_doctor_ignore() {
        let source = "x = 1  # doctor:ignore security/hardcoded-secret reason=\"not real\"\n";
        let suppressions = collect_suppressions(source, "test.py");
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].rule, "security/hardcoded-secret");
        assert_eq!(suppressions[0].reason, "not real");
    }

    #[test]
    fn collect_suppressions_ignores_plain_comments() {
        let source = "x = 1  # this is a normal comment\n";
        let suppressions = collect_suppressions(source, "test.py");
        assert!(suppressions.is_empty());
    }

    #[test]
    fn line_suppresses_rule_with_noqa() {
        assert!(line_suppresses_rule(
            "x = 1  # noqa: security/unsafe-yaml-load",
            "security/unsafe-yaml-load"
        ));
    }

    #[test]
    fn line_suppresses_rule_with_blanket_noqa() {
        assert!(line_suppresses_rule("x = 1  # noqa", "anything/rule"));
    }

    #[test]
    fn line_suppresses_rule_with_wildcard() {
        assert!(line_suppresses_rule(
            "x = 1  # noqa: security/*",
            "security/unsafe-yaml-load"
        ));
    }

    #[test]
    fn line_does_not_suppress_unrelated_rule() {
        assert!(!line_suppresses_rule(
            "x = 1  # noqa: architecture/giant-function",
            "security/unsafe-yaml-load"
        ));
    }

    #[test]
    fn selector_matches_exact() {
        assert!(selector_matches(
            "security/unsafe-yaml-load",
            "security/unsafe-yaml-load"
        ));
        assert!(!selector_matches(
            "security/unsafe-yaml-load",
            "security/cors-wildcard"
        ));
    }

    #[test]
    fn selector_matches_wildcard() {
        assert!(selector_matches("security/unsafe-yaml-load", "security/*"));
        assert!(selector_matches("security/unsafe-yaml-load", "security/"));
        assert!(!selector_matches(
            "architecture/giant-function",
            "security/*"
        ));
    }
}
