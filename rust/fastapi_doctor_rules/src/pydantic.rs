use rustpython_parser::ast::{self, Expr, Stmt};
use std::collections::{HashMap, HashSet};

use crate::engine::RuleSelection;
use fastapi_doctor_core::ast_helpers::*;
use fastapi_doctor_core::{Config, Issue, ModuleIndex};

fn is_base_model_class(node: &ast::StmtClassDef) -> bool {
    node.bases.iter().any(|base| match base {
        Expr::Name(n) => n.id.as_str() == "BaseModel",
        Expr::Attribute(a) => a.attr.as_str() == "BaseModel",
        _ => false,
    })
}

fn is_sensitive_field_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "api_key",
        "apikey",
        "password",
        "secret",
        "auth_token",
        "authtoken",
        "credential",
        "private_key",
        "privatekey",
    ]
    .iter()
    .any(|p| lower.contains(p))
}

fn annotation_contains_secret_str(ann: &Expr) -> bool {
    match ann {
        Expr::Name(n) => n.id.as_str() == "SecretStr",
        Expr::Attribute(a) => a.attr.as_str() == "SecretStr",
        Expr::Subscript(s) => annotation_contains_secret_str(&s.value),
        _ => {
            // Fallback: dump the AST fragment and check for SecretStr
            let mut found = false;
            walk_expr_tree(ann, &mut |expr| {
                if matches!(expr, Expr::Name(n) if n.id.as_str() == "SecretStr") {
                    found = true;
                }
                if matches!(expr, Expr::Attribute(a) if a.attr.as_str() == "SecretStr") {
                    found = true;
                }
            });
            found
        }
    }
}

fn normalized_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

fn field_alias_strings(value: &Expr) -> Vec<String> {
    let Expr::Call(call) = value else {
        return Vec::new();
    };
    let is_field = matches!(&*call.func, Expr::Name(name) if name.id.as_str() == "Field")
        || matches!(&*call.func, Expr::Attribute(attr) if attr.attr.as_str() == "Field");
    if !is_field {
        return Vec::new();
    }

    let mut labels = Vec::new();
    for keyword in &call.keywords {
        match keyword.arg.as_deref() {
            Some("alias") | Some("serialization_alias") => {
                if let Expr::Constant(constant) = &keyword.value {
                    if let ast::Constant::Str(label) = &constant.value {
                        labels.push(label.to_string());
                    }
                }
            }
            Some("validation_alias") => match &keyword.value {
                Expr::Constant(constant) => {
                    if let ast::Constant::Str(label) = &constant.value {
                        labels.push(label.to_string());
                    }
                }
                Expr::Call(alias_call) => {
                    let is_alias_choices = matches!(
                        &*alias_call.func,
                        Expr::Name(name) if name.id.as_str() == "AliasChoices"
                    ) || matches!(
                        &*alias_call.func,
                        Expr::Attribute(attr) if attr.attr.as_str() == "AliasChoices"
                    );
                    if is_alias_choices {
                        for arg in &alias_call.args {
                            if let Expr::Constant(constant) = arg {
                                if let ast::Constant::Str(label) = &constant.value {
                                    labels.push(label.to_string());
                                }
                            }
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    labels
}

fn looks_like_constructor_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(name)
            if name
                .id
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_uppercase()) =>
        {
            Some(name.id.to_string())
        }
        Expr::Attribute(attr)
            if attr
                .attr
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_uppercase()) =>
        {
            Some(attr.attr.to_string())
        }
        _ => None,
    }
}

pub(crate) fn collect_pydantic_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
    rules: &RuleSelection,
    config: &Config,
) -> Vec<Issue> {
    let mut issues = Vec::new();
    let base_model_names: HashSet<String> = suite
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::ClassDef(node) if is_base_model_class(node) => Some(node.name.to_string()),
            _ => None,
        })
        .collect();

    // Collect TYPE_CHECKING class names
    let mut type_checking_names: HashSet<String> = HashSet::new();
    if module.source.contains("TYPE_CHECKING") {
        walk_suite_stmts(suite, &mut |stmt| {
            if let Stmt::If(node) = stmt {
                if matches!(&*node.test, Expr::Name(n) if n.id.as_str() == "TYPE_CHECKING") {
                    walk_suite_stmts(&node.body, &mut |inner| {
                        if let Stmt::ClassDef(cls) = inner {
                            type_checking_names.insert(cls.name.to_string());
                        }
                    });
                }
            }
        });
    }

    let api_boundary_dirs: HashSet<&str> = [
        "routers",
        "router",
        "interfaces",
        "interface",
        "schemas",
        "schema",
        "endpoints",
        "endpoint",
        "api",
        "views",
    ]
    .into_iter()
    .collect();
    let internal_dirs: HashSet<&str> = [
        "services",
        "service",
        "utils",
        "util",
        "helpers",
        "helper",
        "internal",
        "core",
        "domain",
        "agents",
        "agent",
        "state",
        "workflows",
        "workflow",
        "lib",
        "scripts",
        "script",
        "tests",
        "test",
        "migrations",
        "middleware",
    ]
    .into_iter()
    .collect();
    let is_at_boundary = module
        .path_parts
        .iter()
        .any(|p| api_boundary_dirs.contains(p.to_ascii_lowercase().as_str()));
    let is_internal = module
        .path_parts
        .iter()
        .any(|p| internal_dirs.contains(p.to_ascii_lowercase().as_str()));
    let everywhere = config.should_be_model_mode == "everywhere";

    walk_suite_stmts(suite, &mut |stmt| {
        let Stmt::ClassDef(node) = stmt else { return };
        let class_name = node.name.as_str();

        if type_checking_names.contains(class_name) {
            return;
        }

        let is_model = is_base_model_class(node);

        // pydantic/sensitive-field-type and pydantic/secretstr
        if is_model && (rules.sensitive_field_type || rules.pydantic_secretstr) {
            for body_stmt in &node.body {
                if let Stmt::AnnAssign(ann) = body_stmt {
                    if let Expr::Name(target) = &*ann.target {
                        if is_sensitive_field_name(target.id.as_str())
                            && !annotation_contains_secret_str(&ann.annotation)
                        {
                            let line = module.line_for_offset(ann.range.start().to_usize());
                            if rules.sensitive_field_type {
                                issues.push(Issue {
                                    check: "pydantic/sensitive-field-type",
                                    severity: "warning",
                                    category: "Pydantic",
                                    line,
                                    path: module.rel_path.to_string(),
                                    message: Box::leak(
                                        format!(
                                            "Sensitive field '{}' in model '{}' should use SecretStr",
                                            target.id, class_name
                                        )
                                        .into_boxed_str(),
                                    ),
                                    help: "Use pydantic.SecretStr to prevent accidental leakage in logs or JSON.",
                                });
                            } else {
                                issues.push(Issue {
                                    check: "security/pydantic-secretstr",
                                    severity: "warning",
                                    category: "Security",
                                    line,
                                    path: module.rel_path.to_string(),
                                    message: Box::leak(
                                        format!(
                                            "Field '{}' in model '{}' should use SecretStr",
                                            target.id, class_name
                                        )
                                        .into_boxed_str(),
                                    ),
                                    help: "Use pydantic.SecretStr for sensitive fields to prevent leakage. Access the value via .get_secret_value().",
                                });
                            }
                        }
                    }
                }
            }
        }

        // pydantic/mutable-default
        if is_model && rules.mutable_model_default {
            for body_stmt in &node.body {
                if let Stmt::AnnAssign(ann) = body_stmt {
                    if ann.value.is_none() {
                        continue;
                    }
                    let val = ann.value.as_ref().unwrap();
                    let is_mutable = matches!(&**val, Expr::List(l) if l.elts.is_empty())
                        || matches!(&**val, Expr::Dict(d) if d.keys.is_empty())
                        || matches!(&**val, Expr::Set(s) if s.elts.is_empty())
                        || matches!(&**val, Expr::Call(c) if matches!(&*c.func, Expr::Name(n) if matches!(n.id.as_str(), "list" | "dict" | "set")) && c.args.is_empty());
                    if is_mutable {
                        let line = module.line_for_offset(ann.range.start().to_usize());
                        issues.push(Issue {
                            check: "pydantic/mutable-default",
                            severity: "error",
                            category: "Pydantic",
                            line,
                            path: module.rel_path.to_string(),
                            message: Box::leak(
                                format!(
                                    "Mutable default in model '{}' — use Field(default_factory=...)",
                                    class_name
                                )
                                .into_boxed_str(),
                            ),
                            help: "Replace `field: list[X] = []` with `field: list[X] = Field(default_factory=list)`.",
                        });
                    }
                }
            }
        }

        if is_model && rules.normalized_name_collision {
            let class_line = module.line_for_offset(node.range.start().to_usize());
            let mut seen: HashMap<String, Vec<(String, String, usize)>> = HashMap::new();

            for body_stmt in &node.body {
                let Stmt::AnnAssign(ann) = body_stmt else {
                    continue;
                };
                let Expr::Name(target) = &*ann.target else {
                    continue;
                };

                let field_name = target.id.to_string();
                let mut labels = vec![field_name.clone()];
                if let Some(value) = ann.value.as_ref() {
                    labels.extend(field_alias_strings(value));
                }

                for label in labels {
                    let normalized = normalized_name(&label);
                    if normalized.is_empty() {
                        continue;
                    }
                    seen.entry(normalized).or_default().push((
                        field_name.clone(),
                        label,
                        module.line_for_offset(ann.range.start().to_usize()),
                    ));
                }
            }

            for entries in seen.values() {
                let distinct_fields: HashSet<&str> =
                    entries.iter().map(|(field, _, _)| field.as_str()).collect();
                if distinct_fields.len() < 2 {
                    continue;
                }

                let mut rendered = Vec::new();
                for (field, label, _) in entries {
                    if field == label {
                        rendered.push(field.clone());
                    } else {
                        rendered.push(format!("{field} (alias {label})"));
                    }
                }
                rendered.sort();
                rendered.dedup();

                let line = entries
                    .iter()
                    .map(|(_, _, line)| *line)
                    .min()
                    .unwrap_or(class_line);
                issues.push(Issue {
                    check: "pydantic/normalized-name-collision",
                    severity: "warning",
                    category: "Pydantic",
                    line,
                    path: module.rel_path.to_string(),
                    message: Box::leak(
                        format!(
                            "Model '{}' defines near-duplicate normalized names: {}",
                            class_name,
                            rendered.join(", ")
                        )
                        .into_boxed_str(),
                    ),
                    help: "Keep one canonical field name and express external spelling differences through a single alias on that field.",
                });
            }
        }

        // pydantic/should-be-model
        if rules.should_be_model && !is_model {
            let class_line = module.line_for_offset(node.range.start().to_usize());

            // TypedDict
            let is_typed_dict = node.bases.iter().any(|b| match b {
                Expr::Name(n) => n.id.as_str() == "TypedDict",
                Expr::Attribute(a) => a.attr.as_str() == "TypedDict",
                _ => false,
            });
            if is_typed_dict {
                // Skip total=False
                let has_total_false = node.keywords.iter().any(|kw| {
                    kw.arg.as_deref() == Some("total")
                        && matches!(&kw.value, Expr::Constant(c) if matches!(c.value, ast::Constant::Bool(false)))
                });
                if has_total_false {
                    return;
                }
                let should_flag = everywhere
                    || is_at_boundary
                    || class_name.ends_with("Request")
                    || class_name.ends_with("Response")
                    || class_name.ends_with("Schema")
                    || class_name.ends_with("Payload");
                if should_flag {
                    issues.push(Issue {
                        check: "pydantic/should-be-model",
                        severity: "warning",
                        category: "Pydantic",
                        line: class_line,
                        path: module.rel_path.to_string(),
                        message: Box::leak(
                            format!("TypedDict '{}' should be a Pydantic BaseModel", class_name).into_boxed_str(),
                        ),
                        help: "TypedDicts provide no runtime validation. BaseModel gives you validation, serialization, and OpenAPI schema.",
                    });
                }
                return;
            }

            // NamedTuple
            let is_named_tuple = node.bases.iter().any(|b| match b {
                Expr::Name(n) => n.id.as_str() == "NamedTuple",
                Expr::Attribute(a) => a.attr.as_str() == "NamedTuple",
                _ => false,
            });
            if is_named_tuple {
                let field_count = node
                    .body
                    .iter()
                    .filter(|s| matches!(s, Stmt::AnnAssign(_)))
                    .count();
                let has_api_name = class_name.ends_with("Request")
                    || class_name.ends_with("Response")
                    || class_name.ends_with("Schema")
                    || class_name.ends_with("Payload");
                if field_count <= 3 && !has_api_name {
                    return;
                }
                let should_flag = everywhere || is_at_boundary || has_api_name;
                if should_flag {
                    issues.push(Issue {
                        check: "pydantic/should-be-model",
                        severity: "warning",
                        category: "Pydantic",
                        line: class_line,
                        path: module.rel_path.to_string(),
                        message: Box::leak(
                            format!("NamedTuple '{}' should be a Pydantic BaseModel with frozen=True", class_name).into_boxed_str(),
                        ),
                        help: "BaseModel(frozen=True) provides the same immutability plus validation and OpenAPI support.",
                    });
                }
                return;
            }

            // @dataclass
            let is_dataclass = node.decorator_list.iter().any(|dec| match dec {
                Expr::Name(n) => n.id.as_str() == "dataclass",
                Expr::Attribute(a) => a.attr.as_str() == "dataclass",
                Expr::Call(c) => match &*c.func {
                    Expr::Name(n) => n.id.as_str() == "dataclass",
                    Expr::Attribute(a) => a.attr.as_str() == "dataclass",
                    _ => false,
                },
                _ => false,
            });
            if is_dataclass {
                // Skip slots=True or frozen=True
                let has_slots_or_frozen = node.decorator_list.iter().any(|dec| {
                    if let Expr::Call(c) = dec {
                        c.keywords.iter().any(|kw| {
                            matches!(kw.arg.as_deref(), Some("slots") | Some("frozen"))
                                && matches!(&kw.value, Expr::Constant(c) if matches!(c.value, ast::Constant::Bool(true)))
                        })
                    } else {
                        false
                    }
                });
                if has_slots_or_frozen {
                    return;
                }
                if !everywhere && is_internal {
                    return;
                }
                let has_api_name = class_name.ends_with("Request")
                    || class_name.ends_with("Response")
                    || class_name.ends_with("Schema")
                    || class_name.ends_with("Payload");
                let should_flag = everywhere || is_at_boundary || has_api_name;
                if should_flag {
                    issues.push(Issue {
                        check: "pydantic/should-be-model",
                        severity: "warning",
                        category: "Pydantic",
                        line: class_line,
                        path: module.rel_path.to_string(),
                        message: Box::leak(
                            format!("@dataclass '{}' should be a Pydantic BaseModel", class_name).into_boxed_str(),
                        ),
                        help: "Pydantic provides validation, serialization, and OpenAPI schema generation. Use @dataclass(slots=True) or @dataclass(frozen=True) to exempt.",
                    });
                }
            }
        }
    });

    if rules.normalized_name_collision {
        walk_suite_exprs(suite, &mut |expr| {
            let Expr::Call(call) = expr else { return };
            let Some(callee_name) = looks_like_constructor_name(&call.func) else {
                return;
            };
            if !base_model_names.contains(&callee_name)
                && !call
                    .keywords
                    .iter()
                    .any(|keyword| keyword.arg.as_deref().is_some_and(|arg| arg.contains('_')))
            {
                return;
            }

            let mut seen: HashMap<String, Vec<String>> = HashMap::new();
            for keyword in &call.keywords {
                let Some(arg) = keyword.arg.as_deref() else {
                    continue;
                };
                let normalized = normalized_name(arg);
                if normalized.is_empty() {
                    continue;
                }
                seen.entry(normalized).or_default().push(arg.to_string());
            }

            for entries in seen.values() {
                let distinct: HashSet<&str> = entries.iter().map(String::as_str).collect();
                if distinct.len() < 2 {
                    continue;
                }

                let mut rendered = entries.clone();
                rendered.sort();
                rendered.dedup();
                let line = module.line_for_offset(call.range.start().to_usize());
                issues.push(Issue {
                    check: "pydantic/normalized-name-collision",
                    severity: "warning",
                    category: "Pydantic",
                    line,
                    path: module.rel_path.to_string(),
                    message: Box::leak(
                        format!(
                            "Constructor call '{}(...)' passes near-duplicate keyword names: {}",
                            callee_name,
                            rendered.join(", ")
                        )
                        .into_boxed_str(),
                    ),
                    help: "Normalize on one keyword spelling such as snake_case and map external variants through aliases before object construction.",
                });
            }
        });
    }

    issues
}

// ── Architecture: avoid-sys-exit ────────────────────────────────────────
