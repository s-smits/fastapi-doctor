use rustpython_parser::ast::{self, Expr, Stmt};
use std::collections::HashSet;

use fastapi_doctor_core::ast_helpers::*;
use fastapi_doctor_core::{Issue, ModuleIndex};

pub(crate) fn collect_sql_fstring_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
    if !module.source.contains("text(") {
        return Vec::new();
    }
    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        let is_text = match &*call.func {
            Expr::Name(n) => n.id.as_str() == "text",
            Expr::Attribute(a) => a.attr.as_str() == "text",
            _ => false,
        };
        if !is_text || call.args.is_empty() {
            return;
        }
        if !matches!(&call.args[0], Expr::JoinedStr(_)) {
            return;
        }
        let line = module.line_for_offset(call.range.start().to_usize());
        if module.is_rule_suppressed(line, "security/sql-fstring-interpolation") {
            return;
        }
        // Check nearby lines for noqa: sql-safe or noqa: security
        for check_line in line.saturating_sub(1)..=(line + 1).min(module.lines.len()) {
            if check_line > 0 && check_line <= module.lines.len() {
                let raw = &module.lines[check_line - 1].raw;
                if raw.contains("noqa: sql-safe") || raw.contains("noqa: security") {
                    return;
                }
            }
        }
        issues.push(Issue {
            check: "security/sql-fstring-interpolation",
            severity: "error",
            category: "Security",
            line,
            path: module.rel_path.to_string(),
            message: "SQL injection risk: f-string used inside text() call",
            help: "Keep SQL parameterized instead of interpolating values into text(). Suppress with '# noqa: sql-safe' if trusted.",
        });
    });
    issues
}

pub(crate) fn collect_exception_detail_leak_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    fn looks_like_exception_ref(expr: &Expr) -> bool {
        match expr {
            Expr::Name(name) => matches!(
                name.id.as_str(),
                "exc" | "e" | "err" | "error" | "exception" | "ex"
            ),
            Expr::Attribute(attr) => looks_like_exception_ref(&attr.value),
            _ => false,
        }
    }

    fn detail_uses_exception_value(expr: &Expr, exc_name: Option<&str>) -> bool {
        match expr {
            Expr::Call(detail_call) => {
                matches!(&*detail_call.func, Expr::Name(name) if name.id.as_str() == "str")
                    && detail_call.args.first().is_some_and(|arg| {
                        matches!(arg, Expr::Name(name) if Some(name.id.as_str()) == exc_name)
                            || looks_like_exception_ref(arg)
                    })
            }
            _ => false,
        }
    }

    fn http_exception_detail_leaks(call: &ast::ExprCall, exc_name: Option<&str>) -> bool {
        call.keywords.iter().any(|kw| {
            kw.arg.as_deref() == Some("detail") && detail_uses_exception_value(&kw.value, exc_name)
        })
    }

    if !module.source.contains("HTTPException") {
        return Vec::new();
    }

    let mut issues = Vec::new();
    let mut seen_lines = HashSet::new();
    walk_suite_stmts(suite, &mut |stmt| {
        let handlers = match stmt {
            Stmt::Try(node) => Some(&node.handlers),
            Stmt::TryStar(node) => Some(&node.handlers),
            _ => None,
        };
        let Some(handlers) = handlers else { return };

        for handler in handlers {
            let ast::ExceptHandler::ExceptHandler(handler) = handler;
            let exc_name = handler.name.as_deref();
            let body = &handler.body;
            walk_suite_exprs(body, &mut |expr| {
                let Expr::Call(call) = expr else { return };
                let is_http_exception = match &*call.func {
                    Expr::Name(name) => name.id.as_str() == "HTTPException",
                    Expr::Attribute(attr) => attr.attr.as_str() == "HTTPException",
                    _ => false,
                };
                if !is_http_exception {
                    return;
                }

                if !http_exception_detail_leaks(call, exc_name) {
                    return;
                }
                let line = module.line_for_offset(call.range.start().to_usize());
                if seen_lines.contains(&line)
                    || module.is_rule_suppressed(line, "security/exception-detail-leak")
                {
                    return;
                }
                seen_lines.insert(line);
                issues.push(Issue {
                    check: "security/exception-detail-leak",
                    severity: "warning",
                    category: "Security",
                    line,
                    path: module.rel_path.to_string(),
                    message: "Potential internal error leak in HTTPException detail",
                    help: "Use a generic error message. Log the real exception with logger.exception().",
                });
            });
        }
    });
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        let is_http_exception = match &*call.func {
            Expr::Name(name) => name.id.as_str() == "HTTPException",
            Expr::Attribute(attr) => attr.attr.as_str() == "HTTPException",
            _ => false,
        };
        let leaks_via_str_call = call.keywords.iter().any(|kw| {
            if kw.arg.as_deref() != Some("detail") {
                return false;
            }
            match &kw.value {
                Expr::Call(detail_call) => {
                    matches!(&*detail_call.func, Expr::Name(name) if name.id.as_str() == "str")
                        && detail_call
                            .args
                            .first()
                            .is_some_and(looks_like_exception_ref)
                }
                _ => false,
            }
        });
        if !is_http_exception || !leaks_via_str_call {
            return;
        }
        let line = module.line_for_offset(call.range.start().to_usize());
        if seen_lines.contains(&line)
            || module.is_rule_suppressed(line, "security/exception-detail-leak")
        {
            return;
        }
        seen_lines.insert(line);
        issues.push(Issue {
            check: "security/exception-detail-leak",
            severity: "warning",
            category: "Security",
            line,
            path: module.rel_path.to_string(),
            message: "Potential internal error leak in HTTPException detail",
            help: "Use a generic error message. Log the real exception with logger.exception().",
        });
    });
    issues
}

// ── Security: Hardcoded secrets ─────────────────────────────────────────

const SECRET_PREFIXES: &[&str] = &[
    "sk_live_",
    "sk_test_",
    "AKIA",
    "ghp_",
    "github_pat_",
    "glpat-",
    "xox",
    "sk-",
    "eyJ",
];

fn value_looks_like_identifier(val: &str) -> bool {
    val.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        || val.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn value_looks_like_placeholder(val: &str) -> bool {
    let lower = val.to_ascii_lowercase();
    [
        "fake",
        "test",
        "example",
        "dummy",
        "mock",
        "placeholder",
        "encrypted",
        "sample",
        "your",
        "change",
    ]
    .iter()
    .any(|p| lower.contains(p))
}

fn value_looks_like_real_secret(val: &str) -> bool {
    if value_looks_like_identifier(val) || value_looks_like_placeholder(val) {
        return false;
    }
    if val.starts_with("http://") || val.starts_with("https://") || val.starts_with("ws://") {
        return false;
    }
    let has_upper = val.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = val.chars().any(|c| c.is_ascii_lowercase());
    let has_digit = val.chars().any(|c| c.is_ascii_digit());
    let has_special = val
        .chars()
        .any(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-');
    let char_classes = [has_upper, has_lower, has_digit, has_special]
        .iter()
        .filter(|&&b| b)
        .count();
    char_classes >= 3 || (char_classes >= 2 && val.len() >= 24)
}

fn is_secret_var_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "api_key",
        "apikey",
        "secret_key",
        "secretkey",
        "auth_token",
        "authtoken",
        "password",
        "credential",
        "private_key",
        "privatekey",
    ]
    .iter()
    .any(|p| lower.contains(p))
}

pub(crate) fn collect_hardcoded_secret_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if module.rel_path.contains("tests/") || module.rel_path.contains("test_") {
        return Vec::new();
    }
    let false_positives: HashSet<&str> = [
        "",
        "changeme",
        "xxx",
        "your-api-key",
        "CHANGE_ME",
        "TODO",
        "placeholder",
        "test",
        "dummy",
        "fake",
        "mock",
        "example",
        "none",
        "null",
        "undefined",
        "n/a",
        "na",
    ]
    .into_iter()
    .collect();

    let mut issues = Vec::new();
    walk_suite_stmts(suite, &mut |stmt| {
        let Stmt::Assign(node) = stmt else { return };
        let Expr::Constant(val) = &*node.value else {
            return;
        };
        let ast::Constant::Str(val_str) = &val.value else {
            return;
        };
        let val_str = val_str.as_str();
        if val_str.is_empty()
            || val_str.len() < 8
            || false_positives.contains(val_str.to_ascii_lowercase().as_str())
        {
            return;
        }
        let line = module.line_for_offset(node.range.start().to_usize());
        if module.is_rule_suppressed(line, "security/hardcoded-secret") {
            return;
        }
        // Check 1: Known secret patterns
        let value_match = SECRET_PREFIXES
            .iter()
            .any(|prefix| val_str.starts_with(prefix));
        if value_match {
            issues.push(Issue {
                check: "security/hardcoded-secret",
                severity: "error",
                category: "Security",
                line,
                path: module.rel_path.to_string(),
                message: "Hardcoded secret detected — use environment variables or a secrets manager",
                help: "Move secrets to environment variables: os.environ['KEY'] or a secrets manager like AWS SM / Vault.",
            });
            return;
        }
        // Check 2: Variable name suggests secret + value has entropy
        for target in &node.targets {
            let name = match target {
                Expr::Name(n) => n.id.as_str(),
                Expr::Attribute(a) => a.attr.as_str(),
                _ => continue,
            };
            if is_secret_var_name(name) && value_looks_like_real_secret(val_str) {
                issues.push(Issue {
                    check: "security/hardcoded-secret",
                    severity: "error",
                    category: "Security",
                    line,
                    path: module.rel_path.to_string(),
                    message: Box::leak(
                        format!("Variable '{}' looks like a secret with a hardcoded string value", name)
                            .into_boxed_str(),
                    ),
                    help: "Move secrets to environment variables or a secrets manager. Never commit real credentials.",
                });
                break;
            }
        }
    });
    issues
}

// ── Pydantic rules ──────────────────────────────────────────────────────
