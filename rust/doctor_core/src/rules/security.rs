use std::collections::HashSet;
use rustpython_parser::ast::{self, Expr, Stmt};

use crate::{Issue, ModuleIndex};
use crate::ast_helpers::*;

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
            path: module.rel_path.clone(),
            message: "SQL injection risk: f-string used inside text() call",
            help: "Keep SQL parameterized instead of interpolating values into text(). Suppress with '# noqa: sql-safe' if trusted.",
        });
    });
    issues
}

// ── Security: Hardcoded secrets ─────────────────────────────────────────

const SECRET_PREFIXES: &[&str] = &[
    "sk_live_", "sk_test_", "AKIA", "ghp_", "github_pat_", "glpat-", "xox",
    "sk-", "eyJ",
];

fn value_looks_like_identifier(val: &str) -> bool {
    val.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        || val.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn value_looks_like_placeholder(val: &str) -> bool {
    let lower = val.to_ascii_lowercase();
    ["fake", "test", "example", "dummy", "mock", "placeholder", "encrypted", "sample", "your", "change"]
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
    let has_special = val.chars().any(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-');
    let char_classes = [has_upper, has_lower, has_digit, has_special]
        .iter()
        .filter(|&&b| b)
        .count();
    char_classes >= 3 || (char_classes >= 2 && val.len() >= 24)
}

fn is_secret_var_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["api_key", "apikey", "secret_key", "secretkey", "auth_token", "authtoken", "password", "credential", "private_key", "privatekey"]
        .iter()
        .any(|p| lower.contains(p))
}

pub(crate) fn collect_hardcoded_secret_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
    if module.rel_path.contains("tests/") || module.rel_path.contains("test_") {
        return Vec::new();
    }
    let false_positives: HashSet<&str> = [
        "", "changeme", "xxx", "your-api-key", "CHANGE_ME", "TODO",
        "placeholder", "test", "dummy", "fake", "mock", "example",
        "none", "null", "undefined", "n/a", "na",
    ]
    .into_iter()
    .collect();

    let mut issues = Vec::new();
    walk_suite_stmts(suite, &mut |stmt| {
        let Stmt::Assign(node) = stmt else { return };
        let Expr::Constant(val) = &*node.value else { return };
        let ast::Constant::Str(val_str) = &val.value else { return };
        let val_str = val_str.as_str();
        if val_str.is_empty() || val_str.len() < 8 || false_positives.contains(val_str.to_ascii_lowercase().as_str()) {
            return;
        }
        let line = module.line_for_offset(node.range.start().to_usize());
        if module.is_rule_suppressed(line, "security/hardcoded-secret") {
            return;
        }
        // Check 1: Known secret patterns
        let value_match = SECRET_PREFIXES.iter().any(|prefix| val_str.starts_with(prefix));
        if value_match {
            issues.push(Issue {
                check: "security/hardcoded-secret",
                severity: "error",
                category: "Security",
                line,
                path: module.rel_path.clone(),
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
                    path: module.rel_path.clone(),
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

