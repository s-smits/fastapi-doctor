use std::collections::HashMap;

use rustpython_parser::ast::{self, Constant, Expr, Ranged, Stmt};

use crate::ModuleIndex;

pub const AWAITABLE_RETURN_CALLS: &[&str] = &["create_task", "ensure_future", "gather", "shield"];
pub const SYNC_HTTP_ATTRS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "request"];
pub const ASYNC_HELPER_MAX_DEPTH: usize = 5;

#[derive(Clone)]
pub enum Callee {
    Name(String),
    SelfMethod(String),
    Attribute { base: Option<String>, attr: String },
    Other,
}

#[derive(Clone)]
pub struct CallSite {
    pub line: usize,
    pub callee: Callee,
}

#[derive(Default)]
pub struct FunctionBodyCollector {
    pub direct_calls: Vec<CallSite>,
    pub await_calls: Vec<CallSite>,
    pub async_for_calls: Vec<CallSite>,
    pub async_with_calls: Vec<CallSite>,
    pub has_async_constructs: bool,
    pub has_async_for_or_with: bool,
    pub is_generator: bool,
    pub returns_awaitable: bool,
}

#[derive(Clone)]
pub struct FunctionContext {
    pub name: String,
    pub qualname: String,
    pub owner_class: Option<String>,
    pub is_async: bool,
    pub is_route_handler: bool,
    pub has_async_constructs: bool,
    pub is_generator: bool,
    pub is_sync_context_manager: bool,
    pub returns_awaitable: bool,
    pub line: usize,
    pub direct_calls: Vec<CallSite>,
    pub await_calls: Vec<CallSite>,
    pub async_for_calls: Vec<CallSite>,
    pub async_with_calls: Vec<CallSite>,
    pub has_async_for_or_with: bool,
    pub is_stub_body: bool,
    pub is_abstractmethod: bool,
    pub owner_is_protocol: bool,
}

pub struct FunctionIndex {
    pub functions: Vec<FunctionContext>,
    pub by_name: HashMap<String, usize>,
    pub by_method: HashMap<(String, String), usize>,
}

impl FunctionIndex {
    pub fn from_suite(module: &ModuleIndex, suite: &ast::Suite) -> Self {
        let mut functions = Vec::new();
        for stmt in suite {
            match stmt {
                Stmt::FunctionDef(node) => {
                    functions.push(build_function_context(module, node, None, false))
                }
                Stmt::AsyncFunctionDef(node) => {
                    functions.push(build_async_function_context(module, node, None, false))
                }
                Stmt::ClassDef(node) => {
                    let owner = node.name.to_string();
                    let owner_is_protocol = class_extends_protocol(node);
                    for class_stmt in &node.body {
                        match class_stmt {
                            Stmt::FunctionDef(method) => {
                                functions.push(build_function_context(
                                    module,
                                    method,
                                    Some(owner.clone()),
                                    owner_is_protocol,
                                ));
                            }
                            Stmt::AsyncFunctionDef(method) => {
                                functions.push(build_async_function_context(
                                    module,
                                    method,
                                    Some(owner.clone()),
                                    owner_is_protocol,
                                ));
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        let mut by_name = HashMap::new();
        let mut by_method = HashMap::new();
        for (idx, function) in functions.iter().enumerate() {
            if let Some(owner_class) = &function.owner_class {
                by_method.insert((owner_class.clone(), function.name.clone()), idx);
            } else {
                by_name.insert(function.name.clone(), idx);
            }
        }

        Self {
            functions,
            by_name,
            by_method,
        }
    }

    pub fn get_context(&self, qualname: &str) -> Option<&FunctionContext> {
        if let Some((owner, name)) = qualname.rsplit_once('.') {
            self.by_method
                .get(&(owner.to_string(), name.to_string()))
                .map(|idx| &self.functions[*idx])
        } else {
            self.by_name.get(qualname).map(|idx| &self.functions[*idx])
        }
    }

    pub fn resolve_call<'a>(
        &'a self,
        caller: &'a FunctionContext,
        call: &CallSite,
    ) -> Option<&'a FunctionContext> {
        match &call.callee {
            Callee::Name(name) => self.by_name.get(name).map(|idx| &self.functions[*idx]),
            Callee::SelfMethod(name) => caller.owner_class.as_ref().and_then(|owner_class| {
                self.by_method
                    .get(&(owner_class.clone(), name.clone()))
                    .map(|idx| &self.functions[*idx])
            }),
            _ => None,
        }
    }
}

pub fn build_function_context(
    module: &ModuleIndex,
    node: &ast::StmtFunctionDef,
    owner_class: Option<String>,
    owner_is_protocol: bool,
) -> FunctionContext {
    build_context_common(
        module,
        &node.name.to_string(),
        node.range(),
        &node.body,
        &node.decorator_list,
        owner_class,
        false,
        owner_is_protocol,
    )
}

pub fn build_async_function_context(
    module: &ModuleIndex,
    node: &ast::StmtAsyncFunctionDef,
    owner_class: Option<String>,
    owner_is_protocol: bool,
) -> FunctionContext {
    build_context_common(
        module,
        &node.name.to_string(),
        node.range(),
        &node.body,
        &node.decorator_list,
        owner_class,
        true,
        owner_is_protocol,
    )
}

pub fn build_context_common(
    module: &ModuleIndex,
    name: &str,
    range: rustpython_parser::ast::text_size::TextRange,
    body: &[Stmt],
    decorators: &[Expr],
    owner_class: Option<String>,
    is_async: bool,
    owner_is_protocol: bool,
) -> FunctionContext {
    let mut collector = FunctionBodyCollector::default();
    for stmt in body {
        walk_stmt(module, stmt, owner_class.as_deref(), &mut collector);
    }

    let qualname = owner_class
        .as_ref()
        .map_or_else(|| name.to_string(), |owner| format!("{owner}.{name}"));
    FunctionContext {
        name: name.to_string(),
        qualname,
        owner_class,
        is_async,
        is_route_handler: decorators
            .iter()
            .any(|decorator| looks_like_route_handler(module, decorator)),
        has_async_constructs: collector.has_async_constructs,
        is_generator: collector.is_generator,
        is_sync_context_manager: decorators
            .iter()
            .any(|decorator| looks_like_context_manager(decorator, false)),
        returns_awaitable: collector.returns_awaitable,
        line: module.line_for_offset(range.start().to_usize()),
        direct_calls: collector.direct_calls,
        await_calls: collector.await_calls,
        async_for_calls: collector.async_for_calls,
        async_with_calls: collector.async_with_calls,
        has_async_for_or_with: collector.has_async_for_or_with,
        is_stub_body: is_stub_body(body),
        is_abstractmethod: decorators
            .iter()
            .any(|decorator| decorator_name(decorator).as_deref() == Some("abstractmethod")),
        owner_is_protocol,
    }
}

fn class_extends_protocol(node: &ast::StmtClassDef) -> bool {
    node.bases.iter().any(expr_mentions_protocol)
}

fn expr_mentions_protocol(expr: &Expr) -> bool {
    match expr {
        Expr::Name(name) => name.id.as_str() == "Protocol",
        Expr::Attribute(attr) => attr.attr.as_str() == "Protocol",
        Expr::Subscript(subscript) => expr_mentions_protocol(&subscript.value),
        _ => false,
    }
}

fn is_stub_body(body: &[Stmt]) -> bool {
    let body = strip_docstring_stmt(body);
    matches!(body, [] | [Stmt::Pass(_)])
        || matches!(
            body,
            [Stmt::Expr(expr)]
                if matches!(&*expr.value, Expr::Constant(node) if matches!(node.value, Constant::Ellipsis))
        )
}

fn strip_docstring_stmt(body: &[Stmt]) -> &[Stmt] {
    if body.first().is_some_and(|stmt| {
        matches!(
            stmt,
            Stmt::Expr(expr)
                if matches!(&*expr.value, Expr::Constant(node) if matches!(node.value, Constant::Str(_)))
        )
    }) {
        &body[1..]
    } else {
        body
    }
}

pub fn looks_like_route_handler(module: &ModuleIndex, decorator: &Expr) -> bool {
    let source = module.source_slice(decorator.range()).to_ascii_lowercase();
    source.contains("router") || source.contains("app")
}

pub fn looks_like_context_manager(decorator: &Expr, async_only: bool) -> bool {
    let Some(name) = decorator_name(decorator) else {
        return false;
    };
    let lowered = name.to_ascii_lowercase();
    if async_only {
        lowered.contains("asynccontextmanager")
    } else {
        lowered.contains("contextmanager") && !lowered.contains("async")
    }
}

pub fn decorator_name(decorator: &Expr) -> Option<String> {
    match decorator {
        Expr::Name(node) => Some(node.id.to_string()),
        Expr::Attribute(node) => Some(node.attr.to_string()),
        Expr::Call(node) => decorator_name(&node.func),
        _ => None,
    }
}

pub fn walk_stmt(
    module: &ModuleIndex,
    stmt: &Stmt,
    owner_class: Option<&str>,
    collector: &mut FunctionBodyCollector,
) {
    match stmt {
        Stmt::FunctionDef(_) | Stmt::AsyncFunctionDef(_) | Stmt::ClassDef(_) => {}
        Stmt::Return(node) => {
            if let Some(value) = &node.value {
                if returns_awaitable_call(value) {
                    collector.returns_awaitable = true;
                }
                walk_expr(module, value, owner_class, collector);
            }
        }
        Stmt::AsyncFor(node) => {
            collector.has_async_constructs = true;
            collector.has_async_for_or_with = true;
            if let Expr::Call(call) = &*node.iter {
                collector
                    .async_for_calls
                    .push(call_site(module, call, owner_class));
            }
            walk_expr(module, &node.target, owner_class, collector);
            walk_expr(module, &node.iter, owner_class, collector);
            for inner in &node.body {
                walk_stmt(module, inner, owner_class, collector);
            }
            for inner in &node.orelse {
                walk_stmt(module, inner, owner_class, collector);
            }
        }
        Stmt::AsyncWith(node) => {
            collector.has_async_constructs = true;
            collector.has_async_for_or_with = true;
            for item in &node.items {
                if let Expr::Call(call) = &item.context_expr {
                    collector
                        .async_with_calls
                        .push(call_site(module, call, owner_class));
                }
                walk_expr(module, &item.context_expr, owner_class, collector);
                if let Some(optional_vars) = &item.optional_vars {
                    walk_expr(module, optional_vars, owner_class, collector);
                }
            }
            for inner in &node.body {
                walk_stmt(module, inner, owner_class, collector);
            }
        }
        Stmt::For(node) => {
            walk_expr(module, &node.target, owner_class, collector);
            walk_expr(module, &node.iter, owner_class, collector);
            for inner in &node.body {
                walk_stmt(module, inner, owner_class, collector);
            }
            for inner in &node.orelse {
                walk_stmt(module, inner, owner_class, collector);
            }
        }
        Stmt::While(node) => {
            walk_expr(module, &node.test, owner_class, collector);
            for inner in &node.body {
                walk_stmt(module, inner, owner_class, collector);
            }
            for inner in &node.orelse {
                walk_stmt(module, inner, owner_class, collector);
            }
        }
        Stmt::If(node) => {
            walk_expr(module, &node.test, owner_class, collector);
            for inner in &node.body {
                walk_stmt(module, inner, owner_class, collector);
            }
            for inner in &node.orelse {
                walk_stmt(module, inner, owner_class, collector);
            }
        }
        Stmt::With(node) => {
            for item in &node.items {
                walk_expr(module, &item.context_expr, owner_class, collector);
                if let Some(optional_vars) = &item.optional_vars {
                    walk_expr(module, optional_vars, owner_class, collector);
                }
            }
            for inner in &node.body {
                walk_stmt(module, inner, owner_class, collector);
            }
        }
        Stmt::Try(node) => {
            for inner in &node.body {
                walk_stmt(module, inner, owner_class, collector);
            }
            for handler in &node.handlers {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                if let Some(expr) = &handler.type_ {
                    walk_expr(module, expr, owner_class, collector);
                }
                for inner in &handler.body {
                    walk_stmt(module, inner, owner_class, collector);
                }
            }
            for inner in &node.orelse {
                walk_stmt(module, inner, owner_class, collector);
            }
            for inner in &node.finalbody {
                walk_stmt(module, inner, owner_class, collector);
            }
        }
        Stmt::TryStar(node) => {
            for inner in &node.body {
                walk_stmt(module, inner, owner_class, collector);
            }
            for handler in &node.handlers {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                if let Some(expr) = &handler.type_ {
                    walk_expr(module, expr, owner_class, collector);
                }
                for inner in &handler.body {
                    walk_stmt(module, inner, owner_class, collector);
                }
            }
            for inner in &node.orelse {
                walk_stmt(module, inner, owner_class, collector);
            }
            for inner in &node.finalbody {
                walk_stmt(module, inner, owner_class, collector);
            }
        }
        Stmt::Assign(node) => {
            for target in &node.targets {
                walk_expr(module, target, owner_class, collector);
            }
            walk_expr(module, &node.value, owner_class, collector);
        }
        Stmt::AnnAssign(node) => {
            walk_expr(module, &node.target, owner_class, collector);
            walk_expr(module, &node.annotation, owner_class, collector);
            if let Some(value) = &node.value {
                walk_expr(module, value, owner_class, collector);
            }
        }
        Stmt::AugAssign(node) => {
            walk_expr(module, &node.target, owner_class, collector);
            walk_expr(module, &node.value, owner_class, collector);
        }
        Stmt::Expr(node) => {
            walk_expr(module, &node.value, owner_class, collector);
        }
        Stmt::Match(node) => {
            walk_expr(module, &node.subject, owner_class, collector);
            for case in &node.cases {
                if let Some(guard) = &case.guard {
                    walk_expr(module, guard, owner_class, collector);
                }
                for inner in &case.body {
                    walk_stmt(module, inner, owner_class, collector);
                }
            }
        }
        Stmt::Raise(node) => {
            if let Some(expr) = &node.exc {
                walk_expr(module, expr, owner_class, collector);
            }
            if let Some(expr) = &node.cause {
                walk_expr(module, expr, owner_class, collector);
            }
        }
        Stmt::Assert(node) => {
            walk_expr(module, &node.test, owner_class, collector);
            if let Some(expr) = &node.msg {
                walk_expr(module, expr, owner_class, collector);
            }
        }
        Stmt::Delete(node) => {
            for target in &node.targets {
                walk_expr(module, target, owner_class, collector);
            }
        }
        Stmt::Import(_)
        | Stmt::ImportFrom(_)
        | Stmt::Pass(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::Global(_)
        | Stmt::Nonlocal(_)
        | Stmt::TypeAlias(_) => {}
    }
}

pub fn walk_expr(
    module: &ModuleIndex,
    expr: &Expr,
    owner_class: Option<&str>,
    collector: &mut FunctionBodyCollector,
) {
    match expr {
        Expr::Lambda(_) => {}
        Expr::Await(node) => {
            collector.has_async_constructs = true;
            if let Expr::Call(call) = &*node.value {
                collector
                    .await_calls
                    .push(call_site(module, call, owner_class));
            }
            walk_expr(module, &node.value, owner_class, collector);
        }
        Expr::Yield(node) => {
            collector.has_async_constructs = true;
            collector.is_generator = true;
            if let Some(value) = &node.value {
                walk_expr(module, value, owner_class, collector);
            }
        }
        Expr::YieldFrom(node) => {
            collector.has_async_constructs = true;
            collector.is_generator = true;
            walk_expr(module, &node.value, owner_class, collector);
        }
        Expr::Call(node) => {
            collector
                .direct_calls
                .push(call_site(module, node, owner_class));
            walk_expr(module, &node.func, owner_class, collector);
            for arg in &node.args {
                walk_expr(module, arg, owner_class, collector);
            }
            for keyword in &node.keywords {
                walk_expr(module, &keyword.value, owner_class, collector);
            }
        }
        Expr::BoolOp(node) => {
            for value in &node.values {
                walk_expr(module, value, owner_class, collector);
            }
        }
        Expr::NamedExpr(node) => {
            walk_expr(module, &node.target, owner_class, collector);
            walk_expr(module, &node.value, owner_class, collector);
        }
        Expr::BinOp(node) => {
            walk_expr(module, &node.left, owner_class, collector);
            walk_expr(module, &node.right, owner_class, collector);
        }
        Expr::UnaryOp(node) => walk_expr(module, &node.operand, owner_class, collector),
        Expr::IfExp(node) => {
            walk_expr(module, &node.test, owner_class, collector);
            walk_expr(module, &node.body, owner_class, collector);
            walk_expr(module, &node.orelse, owner_class, collector);
        }
        Expr::Dict(node) => {
            for key in &node.keys {
                if let Some(key) = key {
                    walk_expr(module, key, owner_class, collector);
                }
            }
            for value in &node.values {
                walk_expr(module, value, owner_class, collector);
            }
        }
        Expr::Set(node) => {
            for elt in &node.elts {
                walk_expr(module, elt, owner_class, collector);
            }
        }
        Expr::ListComp(node) => {
            walk_expr(module, &node.elt, owner_class, collector);
            for generator in &node.generators {
                walk_expr(module, &generator.target, owner_class, collector);
                walk_expr(module, &generator.iter, owner_class, collector);
                for if_expr in &generator.ifs {
                    walk_expr(module, if_expr, owner_class, collector);
                }
            }
        }
        Expr::SetComp(node) => {
            walk_expr(module, &node.elt, owner_class, collector);
            for generator in &node.generators {
                walk_expr(module, &generator.target, owner_class, collector);
                walk_expr(module, &generator.iter, owner_class, collector);
                for if_expr in &generator.ifs {
                    walk_expr(module, if_expr, owner_class, collector);
                }
            }
        }
        Expr::DictComp(node) => {
            walk_expr(module, &node.key, owner_class, collector);
            walk_expr(module, &node.value, owner_class, collector);
            for generator in &node.generators {
                walk_expr(module, &generator.target, owner_class, collector);
                walk_expr(module, &generator.iter, owner_class, collector);
                for if_expr in &generator.ifs {
                    walk_expr(module, if_expr, owner_class, collector);
                }
            }
        }
        Expr::GeneratorExp(node) => {
            walk_expr(module, &node.elt, owner_class, collector);
            for generator in &node.generators {
                walk_expr(module, &generator.target, owner_class, collector);
                walk_expr(module, &generator.iter, owner_class, collector);
                for if_expr in &generator.ifs {
                    walk_expr(module, if_expr, owner_class, collector);
                }
            }
        }
        Expr::Compare(node) => {
            walk_expr(module, &node.left, owner_class, collector);
            for comparator in &node.comparators {
                walk_expr(module, comparator, owner_class, collector);
            }
        }
        Expr::FormattedValue(node) => {
            walk_expr(module, &node.value, owner_class, collector);
            if let Some(format_spec) = &node.format_spec {
                walk_expr(module, format_spec, owner_class, collector);
            }
        }
        Expr::JoinedStr(node) => {
            for value in &node.values {
                walk_expr(module, value, owner_class, collector);
            }
        }
        Expr::Attribute(node) => walk_expr(module, &node.value, owner_class, collector),
        Expr::Subscript(node) => {
            walk_expr(module, &node.value, owner_class, collector);
            walk_expr(module, &node.slice, owner_class, collector);
        }
        Expr::Starred(node) => walk_expr(module, &node.value, owner_class, collector),
        Expr::List(node) => {
            for elt in &node.elts {
                walk_expr(module, elt, owner_class, collector);
            }
        }
        Expr::Tuple(node) => {
            for elt in &node.elts {
                walk_expr(module, elt, owner_class, collector);
            }
        }
        Expr::Slice(node) => {
            if let Some(expr) = &node.lower {
                walk_expr(module, expr, owner_class, collector);
            }
            if let Some(expr) = &node.upper {
                walk_expr(module, expr, owner_class, collector);
            }
            if let Some(expr) = &node.step {
                walk_expr(module, expr, owner_class, collector);
            }
        }
        Expr::Name(_) | Expr::Constant(_) => {}
    }
}

pub fn call_site(
    module: &ModuleIndex,
    call: &ast::ExprCall,
    owner_class: Option<&str>,
) -> CallSite {
    let callee = match &*call.func {
        Expr::Name(node) => Callee::Name(node.id.to_string()),
        Expr::Attribute(node) => {
            if let Expr::Name(base) = &*node.value {
                if matches!(base.id.as_str(), "self" | "cls") && owner_class.is_some() {
                    Callee::SelfMethod(node.attr.to_string())
                } else {
                    Callee::Attribute {
                        base: Some(base.id.to_string()),
                        attr: node.attr.to_string(),
                    }
                }
            } else {
                Callee::Attribute {
                    base: None,
                    attr: node.attr.to_string(),
                }
            }
        }
        _ => Callee::Other,
    };

    CallSite {
        line: module.line_for_offset(call.range.start().to_usize()),
        callee,
    }
}

pub fn returns_awaitable_call(expr: &Expr) -> bool {
    let Expr::Call(call) = expr else {
        return false;
    };
    match &*call.func {
        Expr::Attribute(node) => AWAITABLE_RETURN_CALLS.contains(&node.attr.as_str()),
        Expr::Name(node) => AWAITABLE_RETURN_CALLS.contains(&node.id.as_str()),
        _ => false,
    }
}

pub struct FunctionSpan<'a> {
    pub name: &'a str,
    pub is_async: bool,
    pub range: rustpython_parser::ast::text_size::TextRange,
    pub body: &'a [Stmt],
    pub defaults: Vec<&'a Expr>,
}

pub fn all_functions<'a>(suite: &'a [Stmt]) -> Vec<FunctionSpan<'a>> {
    let mut functions = Vec::new();
    for stmt in suite {
        collect_functions(stmt, &mut functions);
    }
    functions
}

pub fn collect_functions<'a>(stmt: &'a Stmt, functions: &mut Vec<FunctionSpan<'a>>) {
    match stmt {
        Stmt::FunctionDef(node) => {
            functions.push(FunctionSpan {
                name: node.name.as_str(),
                is_async: false,
                range: node.range(),
                body: &node.body,
                defaults: function_default_exprs(&node.args),
            });
            for inner in &node.body {
                collect_functions(inner, functions);
            }
        }
        Stmt::AsyncFunctionDef(node) => {
            functions.push(FunctionSpan {
                name: node.name.as_str(),
                is_async: true,
                range: node.range(),
                body: &node.body,
                defaults: function_default_exprs(&node.args),
            });
            for inner in &node.body {
                collect_functions(inner, functions);
            }
        }
        Stmt::ClassDef(node) => {
            for inner in &node.body {
                collect_functions(inner, functions);
            }
        }
        Stmt::For(node) => {
            for inner in &node.body {
                collect_functions(inner, functions);
            }
            for inner in &node.orelse {
                collect_functions(inner, functions);
            }
        }
        Stmt::AsyncFor(node) => {
            for inner in &node.body {
                collect_functions(inner, functions);
            }
            for inner in &node.orelse {
                collect_functions(inner, functions);
            }
        }
        Stmt::While(node) => {
            for inner in &node.body {
                collect_functions(inner, functions);
            }
            for inner in &node.orelse {
                collect_functions(inner, functions);
            }
        }
        Stmt::If(node) => {
            for inner in &node.body {
                collect_functions(inner, functions);
            }
            for inner in &node.orelse {
                collect_functions(inner, functions);
            }
        }
        Stmt::With(node) => {
            for inner in &node.body {
                collect_functions(inner, functions);
            }
        }
        Stmt::AsyncWith(node) => {
            for inner in &node.body {
                collect_functions(inner, functions);
            }
        }
        Stmt::Try(node) => {
            for inner in &node.body {
                collect_functions(inner, functions);
            }
            for handler in &node.handlers {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                for inner in &handler.body {
                    collect_functions(inner, functions);
                }
            }
            for inner in &node.orelse {
                collect_functions(inner, functions);
            }
            for inner in &node.finalbody {
                collect_functions(inner, functions);
            }
        }
        Stmt::TryStar(node) => {
            for inner in &node.body {
                collect_functions(inner, functions);
            }
            for handler in &node.handlers {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                for inner in &handler.body {
                    collect_functions(inner, functions);
                }
            }
            for inner in &node.orelse {
                collect_functions(inner, functions);
            }
            for inner in &node.finalbody {
                collect_functions(inner, functions);
            }
        }
        Stmt::Match(node) => {
            for case in &node.cases {
                for inner in &case.body {
                    collect_functions(inner, functions);
                }
            }
        }
        _ => {}
    }
}

pub fn walk_suite_stmts<'a>(suite: &'a [Stmt], visit: &mut impl FnMut(&'a Stmt)) {
    for stmt in suite {
        walk_stmt_tree(stmt, visit);
    }
}

pub fn walk_stmt_tree<'a>(stmt: &'a Stmt, visit: &mut impl FnMut(&'a Stmt)) {
    visit(stmt);
    match stmt {
        Stmt::FunctionDef(node) => {
            for inner in &node.body {
                walk_stmt_tree(inner, visit);
            }
        }
        Stmt::AsyncFunctionDef(node) => {
            for inner in &node.body {
                walk_stmt_tree(inner, visit);
            }
        }
        Stmt::ClassDef(node) => {
            for inner in &node.body {
                walk_stmt_tree(inner, visit);
            }
        }
        Stmt::For(node) => {
            walk_expr_tree(&node.target, &mut |_| {});
            walk_expr_tree(&node.iter, &mut |_| {});
            for inner in &node.body {
                walk_stmt_tree(inner, visit);
            }
            for inner in &node.orelse {
                walk_stmt_tree(inner, visit);
            }
        }
        Stmt::AsyncFor(node) => {
            walk_expr_tree(&node.target, &mut |_| {});
            walk_expr_tree(&node.iter, &mut |_| {});
            for inner in &node.body {
                walk_stmt_tree(inner, visit);
            }
            for inner in &node.orelse {
                walk_stmt_tree(inner, visit);
            }
        }
        Stmt::While(node) => {
            walk_expr_tree(&node.test, &mut |_| {});
            for inner in &node.body {
                walk_stmt_tree(inner, visit);
            }
            for inner in &node.orelse {
                walk_stmt_tree(inner, visit);
            }
        }
        Stmt::If(node) => {
            walk_expr_tree(&node.test, &mut |_| {});
            for inner in &node.body {
                walk_stmt_tree(inner, visit);
            }
            for inner in &node.orelse {
                walk_stmt_tree(inner, visit);
            }
        }
        Stmt::With(node) => {
            for item in &node.items {
                walk_expr_tree(&item.context_expr, &mut |_| {});
                if let Some(optional_vars) = &item.optional_vars {
                    walk_expr_tree(optional_vars, &mut |_| {});
                }
            }
            for inner in &node.body {
                walk_stmt_tree(inner, visit);
            }
        }
        Stmt::AsyncWith(node) => {
            for item in &node.items {
                walk_expr_tree(&item.context_expr, &mut |_| {});
                if let Some(optional_vars) = &item.optional_vars {
                    walk_expr_tree(optional_vars, &mut |_| {});
                }
            }
            for inner in &node.body {
                walk_stmt_tree(inner, visit);
            }
        }
        Stmt::Try(node) => {
            for inner in &node.body {
                walk_stmt_tree(inner, visit);
            }
            for handler in &node.handlers {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                if let Some(expr) = &handler.type_ {
                    walk_expr_tree(expr, &mut |_| {});
                }
                for inner in &handler.body {
                    walk_stmt_tree(inner, visit);
                }
            }
            for inner in &node.orelse {
                walk_stmt_tree(inner, visit);
            }
            for inner in &node.finalbody {
                walk_stmt_tree(inner, visit);
            }
        }
        Stmt::TryStar(node) => {
            for inner in &node.body {
                walk_stmt_tree(inner, visit);
            }
            for handler in &node.handlers {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                if let Some(expr) = &handler.type_ {
                    walk_expr_tree(expr, &mut |_| {});
                }
                for inner in &handler.body {
                    walk_stmt_tree(inner, visit);
                }
            }
            for inner in &node.orelse {
                walk_stmt_tree(inner, visit);
            }
            for inner in &node.finalbody {
                walk_stmt_tree(inner, visit);
            }
        }
        Stmt::Match(node) => {
            walk_expr_tree(&node.subject, &mut |_| {});
            for case in &node.cases {
                if let Some(guard) = &case.guard {
                    walk_expr_tree(guard, &mut |_| {});
                }
                for inner in &case.body {
                    walk_stmt_tree(inner, visit);
                }
            }
        }
        _ => {}
    }
}

pub fn walk_suite_exprs<'a>(suite: &'a [Stmt], visit: &mut impl FnMut(&'a Expr)) {
    for stmt in suite {
        walk_stmt_exprs(stmt, visit);
    }
}

pub fn walk_stmt_exprs<'a>(stmt: &'a Stmt, visit: &mut impl FnMut(&'a Expr)) {
    match stmt {
        Stmt::FunctionDef(node) => {
            for decorator in &node.decorator_list {
                walk_expr_tree(decorator, visit);
            }
            if let Some(returns) = &node.returns {
                walk_expr_tree(returns, visit);
            }
            for default in function_default_exprs(&node.args) {
                walk_expr_tree(default, visit);
            }
            for inner in &node.body {
                walk_stmt_exprs(inner, visit);
            }
        }
        Stmt::AsyncFunctionDef(node) => {
            for decorator in &node.decorator_list {
                walk_expr_tree(decorator, visit);
            }
            if let Some(returns) = &node.returns {
                walk_expr_tree(returns, visit);
            }
            for default in function_default_exprs(&node.args) {
                walk_expr_tree(default, visit);
            }
            for inner in &node.body {
                walk_stmt_exprs(inner, visit);
            }
        }
        Stmt::ClassDef(node) => {
            for base in &node.bases {
                walk_expr_tree(base, visit);
            }
            for keyword in &node.keywords {
                walk_expr_tree(&keyword.value, visit);
            }
            for decorator in &node.decorator_list {
                walk_expr_tree(decorator, visit);
            }
            for inner in &node.body {
                walk_stmt_exprs(inner, visit);
            }
        }
        Stmt::Return(node) => {
            if let Some(value) = &node.value {
                walk_expr_tree(value, visit);
            }
        }
        Stmt::Delete(node) => {
            for target in &node.targets {
                walk_expr_tree(target, visit);
            }
        }
        Stmt::Assign(node) => {
            for target in &node.targets {
                walk_expr_tree(target, visit);
            }
            walk_expr_tree(&node.value, visit);
        }
        Stmt::AnnAssign(node) => {
            walk_expr_tree(&node.target, visit);
            walk_expr_tree(&node.annotation, visit);
            if let Some(value) = &node.value {
                walk_expr_tree(value, visit);
            }
        }
        Stmt::AugAssign(node) => {
            walk_expr_tree(&node.target, visit);
            walk_expr_tree(&node.value, visit);
        }
        Stmt::For(node) => {
            walk_expr_tree(&node.target, visit);
            walk_expr_tree(&node.iter, visit);
            for inner in &node.body {
                walk_stmt_exprs(inner, visit);
            }
            for inner in &node.orelse {
                walk_stmt_exprs(inner, visit);
            }
        }
        Stmt::AsyncFor(node) => {
            walk_expr_tree(&node.target, visit);
            walk_expr_tree(&node.iter, visit);
            for inner in &node.body {
                walk_stmt_exprs(inner, visit);
            }
            for inner in &node.orelse {
                walk_stmt_exprs(inner, visit);
            }
        }
        Stmt::While(node) => {
            walk_expr_tree(&node.test, visit);
            for inner in &node.body {
                walk_stmt_exprs(inner, visit);
            }
            for inner in &node.orelse {
                walk_stmt_exprs(inner, visit);
            }
        }
        Stmt::If(node) => {
            walk_expr_tree(&node.test, visit);
            for inner in &node.body {
                walk_stmt_exprs(inner, visit);
            }
            for inner in &node.orelse {
                walk_stmt_exprs(inner, visit);
            }
        }
        Stmt::With(node) => {
            for item in &node.items {
                walk_expr_tree(&item.context_expr, visit);
                if let Some(optional_vars) = &item.optional_vars {
                    walk_expr_tree(optional_vars, visit);
                }
            }
            for inner in &node.body {
                walk_stmt_exprs(inner, visit);
            }
        }
        Stmt::AsyncWith(node) => {
            for item in &node.items {
                walk_expr_tree(&item.context_expr, visit);
                if let Some(optional_vars) = &item.optional_vars {
                    walk_expr_tree(optional_vars, visit);
                }
            }
            for inner in &node.body {
                walk_stmt_exprs(inner, visit);
            }
        }
        Stmt::Try(node) => {
            for inner in &node.body {
                walk_stmt_exprs(inner, visit);
            }
            for handler in &node.handlers {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                if let Some(expr) = &handler.type_ {
                    walk_expr_tree(expr, visit);
                }
                for inner in &handler.body {
                    walk_stmt_exprs(inner, visit);
                }
            }
            for inner in &node.orelse {
                walk_stmt_exprs(inner, visit);
            }
            for inner in &node.finalbody {
                walk_stmt_exprs(inner, visit);
            }
        }
        Stmt::TryStar(node) => {
            for inner in &node.body {
                walk_stmt_exprs(inner, visit);
            }
            for handler in &node.handlers {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                if let Some(expr) = &handler.type_ {
                    walk_expr_tree(expr, visit);
                }
                for inner in &handler.body {
                    walk_stmt_exprs(inner, visit);
                }
            }
            for inner in &node.orelse {
                walk_stmt_exprs(inner, visit);
            }
            for inner in &node.finalbody {
                walk_stmt_exprs(inner, visit);
            }
        }
        Stmt::Assert(node) => {
            walk_expr_tree(&node.test, visit);
            if let Some(msg) = &node.msg {
                walk_expr_tree(msg, visit);
            }
        }
        Stmt::Import(_)
        | Stmt::ImportFrom(_)
        | Stmt::Global(_)
        | Stmt::Nonlocal(_)
        | Stmt::Pass(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::TypeAlias(_) => {}
        Stmt::Expr(node) => walk_expr_tree(&node.value, visit),
        Stmt::Match(node) => {
            walk_expr_tree(&node.subject, visit);
            for case in &node.cases {
                if let Some(guard) = &case.guard {
                    walk_expr_tree(guard, visit);
                }
                for inner in &case.body {
                    walk_stmt_exprs(inner, visit);
                }
            }
        }
        Stmt::Raise(node) => {
            if let Some(exc) = &node.exc {
                walk_expr_tree(exc, visit);
            }
            if let Some(cause) = &node.cause {
                walk_expr_tree(cause, visit);
            }
        }
    }
}

pub fn walk_expr_tree<'a>(expr: &'a Expr, visit: &mut impl FnMut(&'a Expr)) {
    visit(expr);
    match expr {
        Expr::BoolOp(node) => {
            for value in &node.values {
                walk_expr_tree(value, visit);
            }
        }
        Expr::NamedExpr(node) => {
            walk_expr_tree(&node.target, visit);
            walk_expr_tree(&node.value, visit);
        }
        Expr::BinOp(node) => {
            walk_expr_tree(&node.left, visit);
            walk_expr_tree(&node.right, visit);
        }
        Expr::UnaryOp(node) => walk_expr_tree(&node.operand, visit),
        Expr::Lambda(node) => walk_expr_tree(&node.body, visit),
        Expr::IfExp(node) => {
            walk_expr_tree(&node.test, visit);
            walk_expr_tree(&node.body, visit);
            walk_expr_tree(&node.orelse, visit);
        }
        Expr::Dict(node) => {
            for key in &node.keys {
                if let Some(key) = key {
                    walk_expr_tree(key, visit);
                }
            }
            for value in &node.values {
                walk_expr_tree(value, visit);
            }
        }
        Expr::Set(node) => {
            for elt in &node.elts {
                walk_expr_tree(elt, visit);
            }
        }
        Expr::ListComp(node) => {
            walk_expr_tree(&node.elt, visit);
            for generator in &node.generators {
                walk_expr_tree(&generator.target, visit);
                walk_expr_tree(&generator.iter, visit);
                for if_expr in &generator.ifs {
                    walk_expr_tree(if_expr, visit);
                }
            }
        }
        Expr::SetComp(node) => {
            walk_expr_tree(&node.elt, visit);
            for generator in &node.generators {
                walk_expr_tree(&generator.target, visit);
                walk_expr_tree(&generator.iter, visit);
                for if_expr in &generator.ifs {
                    walk_expr_tree(if_expr, visit);
                }
            }
        }
        Expr::DictComp(node) => {
            walk_expr_tree(&node.key, visit);
            walk_expr_tree(&node.value, visit);
            for generator in &node.generators {
                walk_expr_tree(&generator.target, visit);
                walk_expr_tree(&generator.iter, visit);
                for if_expr in &generator.ifs {
                    walk_expr_tree(if_expr, visit);
                }
            }
        }
        Expr::GeneratorExp(node) => {
            walk_expr_tree(&node.elt, visit);
            for generator in &node.generators {
                walk_expr_tree(&generator.target, visit);
                walk_expr_tree(&generator.iter, visit);
                for if_expr in &generator.ifs {
                    walk_expr_tree(if_expr, visit);
                }
            }
        }
        Expr::Await(node) => walk_expr_tree(&node.value, visit),
        Expr::Yield(node) => {
            if let Some(value) = &node.value {
                walk_expr_tree(value, visit);
            }
        }
        Expr::YieldFrom(node) => walk_expr_tree(&node.value, visit),
        Expr::Compare(node) => {
            walk_expr_tree(&node.left, visit);
            for comparator in &node.comparators {
                walk_expr_tree(comparator, visit);
            }
        }
        Expr::Call(node) => {
            walk_expr_tree(&node.func, visit);
            for arg in &node.args {
                walk_expr_tree(arg, visit);
            }
            for keyword in &node.keywords {
                walk_expr_tree(&keyword.value, visit);
            }
        }
        Expr::FormattedValue(node) => {
            walk_expr_tree(&node.value, visit);
            if let Some(format_spec) = &node.format_spec {
                walk_expr_tree(format_spec, visit);
            }
        }
        Expr::JoinedStr(node) => {
            for value in &node.values {
                walk_expr_tree(value, visit);
            }
        }
        Expr::Attribute(node) => walk_expr_tree(&node.value, visit),
        Expr::Subscript(node) => {
            walk_expr_tree(&node.value, visit);
            walk_expr_tree(&node.slice, visit);
        }
        Expr::Starred(node) => walk_expr_tree(&node.value, visit),
        Expr::Name(_) | Expr::Constant(_) => {}
        Expr::List(node) => {
            for elt in &node.elts {
                walk_expr_tree(elt, visit);
            }
        }
        Expr::Tuple(node) => {
            for elt in &node.elts {
                walk_expr_tree(elt, visit);
            }
        }
        Expr::Slice(node) => {
            if let Some(lower) = &node.lower {
                walk_expr_tree(lower, visit);
            }
            if let Some(upper) = &node.upper {
                walk_expr_tree(upper, visit);
            }
            if let Some(step) = &node.step {
                walk_expr_tree(step, visit);
            }
        }
    }
}

pub fn function_default_exprs<'a>(args: &'a ast::Arguments) -> Vec<&'a Expr> {
    args.defaults()
        .chain(
            args.kwonlyargs
                .iter()
                .filter_map(|arg| arg.default.as_ref().map(|expr| expr.as_ref())),
        )
        .collect()
}

pub fn is_mutable_default(expr: &Expr) -> bool {
    match expr {
        Expr::List(_) | Expr::Dict(_) | Expr::Set(_) => true,
        Expr::Call(call) => {
            matches!(&*call.func, Expr::Name(func) if matches!(func.id.as_str(), "list" | "dict" | "set"))
        }
        _ => false,
    }
}

pub fn module_has_async_def(suite: &[Stmt]) -> bool {
    all_functions(suite)
        .iter()
        .any(|function| function.is_async)
}

pub fn name_main_line_ranges(module: &ModuleIndex, suite: &[Stmt]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    walk_suite_stmts(suite, &mut |stmt| {
        let Stmt::If(node) = stmt else {
            return;
        };
        let Expr::Compare(test) = &*node.test else {
            return;
        };
        if test.ops.len() != 1 || test.comparators.len() != 1 {
            return;
        }
        let left = &*test.left;
        let right = &test.comparators[0];
        let is_name_main = matches!(
            (left, right),
            (Expr::Name(name), Expr::Constant(constant))
                if name.id.as_str() == "__name__"
                    && matches!(constant.value, ast::Constant::Str(ref value) if value == "__main__")
        ) || matches!(
            (left, right),
            (Expr::Constant(constant), Expr::Name(name))
                if matches!(constant.value, ast::Constant::Str(ref value) if value == "__main__")
                    && name.id.as_str() == "__name__"
        );
        if !is_name_main {
            return;
        }
        let start = module.line_for_offset(node.range.start().to_usize());
        let end = module.line_for_offset(node.range.end().to_usize().saturating_sub(1));
        ranges.push((start, end));
    });
    ranges
}

pub fn suite_imports_threading_lock(suite: &[Stmt]) -> bool {
    let mut found = false;
    walk_suite_stmts(suite, &mut |stmt| {
        let Stmt::ImportFrom(node) = stmt else {
            return;
        };
        if node.module.as_deref() == Some("threading")
            && node.names.iter().any(|alias| alias.name.as_str() == "Lock")
        {
            found = true;
        }
    });
    found
}

pub fn max_nesting_depth(body: &[Stmt]) -> usize {
    let mut max_depth = 0;
    for stmt in body {
        max_depth = max_depth.max(max_nesting_depth_stmt(stmt, 0));
    }
    max_depth
}

pub fn max_nesting_depth_stmt(stmt: &Stmt, depth: usize) -> usize {
    let mut max_depth = depth;
    match stmt {
        Stmt::If(node) => {
            max_depth = max_depth.max(max_nesting_in_block(&node.body, depth + 1));
            max_depth = max_depth.max(max_nesting_in_block(&node.orelse, depth + 1));
        }
        Stmt::For(node) => {
            max_depth = max_depth.max(max_nesting_in_block(&node.body, depth + 1));
            max_depth = max_depth.max(max_nesting_in_block(&node.orelse, depth + 1));
        }
        Stmt::While(node) => {
            max_depth = max_depth.max(max_nesting_in_block(&node.body, depth + 1));
            max_depth = max_depth.max(max_nesting_in_block(&node.orelse, depth + 1));
        }
        Stmt::With(node) => {
            max_depth = max_depth.max(max_nesting_in_block(&node.body, depth + 1));
        }
        Stmt::AsyncFor(node) => {
            max_depth = max_depth.max(max_nesting_in_block(&node.body, depth + 1));
            max_depth = max_depth.max(max_nesting_in_block(&node.orelse, depth + 1));
        }
        Stmt::AsyncWith(node) => {
            max_depth = max_depth.max(max_nesting_in_block(&node.body, depth + 1));
        }
        Stmt::Try(node) => {
            max_depth = max_depth.max(max_nesting_in_block(&node.body, depth + 1));
            for handler in &node.handlers {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                max_depth = max_depth.max(max_nesting_in_block(&handler.body, depth));
            }
            max_depth = max_depth.max(max_nesting_in_block(&node.orelse, depth));
            max_depth = max_depth.max(max_nesting_in_block(&node.finalbody, depth));
        }
        Stmt::TryStar(node) => {
            max_depth = max_depth.max(max_nesting_in_block(&node.body, depth + 1));
            for handler in &node.handlers {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                max_depth = max_depth.max(max_nesting_in_block(&handler.body, depth));
            }
            max_depth = max_depth.max(max_nesting_in_block(&node.orelse, depth));
            max_depth = max_depth.max(max_nesting_in_block(&node.finalbody, depth));
        }
        Stmt::FunctionDef(node) => {
            max_depth = max_depth.max(max_nesting_in_block(&node.body, depth));
        }
        Stmt::AsyncFunctionDef(node) => {
            max_depth = max_depth.max(max_nesting_in_block(&node.body, depth));
        }
        Stmt::ClassDef(node) => {
            max_depth = max_depth.max(max_nesting_in_block(&node.body, depth));
        }
        Stmt::Match(node) => {
            for case in &node.cases {
                max_depth = max_depth.max(max_nesting_in_block(&case.body, depth));
            }
        }
        _ => {}
    }
    max_depth
}

pub fn max_nesting_in_block(body: &[Stmt], depth: usize) -> usize {
    let mut max_depth = depth;
    for stmt in body {
        max_depth = max_depth.max(max_nesting_depth_stmt(stmt, depth));
    }
    max_depth
}
