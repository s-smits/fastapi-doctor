use rustpython_parser::ast::{self, Expr, Stmt};
use std::collections::HashSet;

use fastapi_doctor_core::ast_helpers::*;
use fastapi_doctor_core::{Issue, ModuleIndex};

fn call_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(name) => Some(name.id.to_string()),
        Expr::Attribute(attr) => {
            let base = call_name(&attr.value)?;
            Some(format!("{base}.{}", attr.attr))
        }
        _ => None,
    }
}

fn keyword_is_false(call: &ast::ExprCall, name: &str) -> bool {
    call.keywords.iter().any(|kw| {
        kw.arg.as_deref() == Some(name)
            && matches!(&kw.value, Expr::Constant(value) if matches!(value.value, ast::Constant::Bool(false)))
    })
}

fn keyword_is_true(call: &ast::ExprCall, name: &str) -> bool {
    call.keywords.iter().any(|kw| {
        kw.arg.as_deref() == Some(name)
            && matches!(&kw.value, Expr::Constant(value) if matches!(value.value, ast::Constant::Bool(true)))
    })
}

fn keyword_list_contains_str(call: &ast::ExprCall, name: &str, expected: &str) -> bool {
    call.keywords.iter().any(|kw| {
        kw.arg.as_deref() == Some(name)
            && matches!(
                &kw.value,
                Expr::List(list)
                    if list.elts.iter().any(|elt| {
                        matches!(
                            elt,
                            Expr::Constant(value)
                                if matches!(&value.value, ast::Constant::Str(raw) if raw == expected)
                        )
                    })
            )
    })
}

fn keyword_str_equals(call: &ast::ExprCall, name: &str, expected: &str) -> bool {
    call.keywords.iter().any(|kw| {
        kw.arg.as_deref() == Some(name)
            && matches!(
                &kw.value,
                Expr::Constant(value)
                    if matches!(&value.value, ast::Constant::Str(raw) if raw == expected)
            )
    })
}

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

pub(crate) fn collect_unsafe_eval_exec_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if !module.source.contains("eval(") && !module.source.contains("exec(") {
        return Vec::new();
    }

    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        let Some(name) = call_name(&call.func) else {
            return;
        };
        if !matches!(
            name.as_str(),
            "eval" | "exec" | "builtins.eval" | "builtins.exec"
        ) {
            return;
        }
        let line = module.line_for_offset(call.range.start().to_usize());
        if module.is_rule_suppressed(line, "security/unsafe-eval-exec") {
            return;
        }
        issues.push(Issue {
            check: "security/unsafe-eval-exec",
            severity: "error",
            category: "Security",
            line,
            path: module.rel_path.to_string(),
            message: Box::leak(
                format!("{name}() executes dynamic Python code").into_boxed_str(),
            ),
            help: "Do not execute dynamic strings. Use explicit parsers such as json.loads(), ast.literal_eval(), or a typed command registry.",
        });
    });
    issues
}

pub(crate) fn collect_unsafe_pickle_load_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if !module.source.contains("pickle") && !module.source.contains("dill") {
        return Vec::new();
    }

    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        let Some(name) = call_name(&call.func) else {
            return;
        };
        if !matches!(
            name.as_str(),
            "pickle.load"
                | "pickle.loads"
                | "dill.load"
                | "dill.loads"
                | "cloudpickle.load"
                | "cloudpickle.loads"
        ) {
            return;
        }
        let line = module.line_for_offset(call.range.start().to_usize());
        if module.is_rule_suppressed(line, "security/unsafe-pickle-load") {
            return;
        }
        issues.push(Issue {
            check: "security/unsafe-pickle-load",
            severity: "error",
            category: "Security",
            line,
            path: module.rel_path.to_string(),
            message: Box::leak(format!("{name}() can execute code during deserialisation").into_boxed_str()),
            help: "Never unpickle untrusted data. Prefer JSON/Pydantic or a signed, typed serialization format.",
        });
    });
    issues
}

pub(crate) fn collect_http_verify_false_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if !module.source.contains("verify") {
        return Vec::new();
    }

    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        if !keyword_is_false(call, "verify") {
            return;
        }
        let Some(name) = call_name(&call.func) else {
            return;
        };
        let is_http_client = name.starts_with("requests.")
            || name.starts_with("httpx.")
            || name.ends_with(".request")
            || name.ends_with(".get")
            || name.ends_with(".post")
            || name.ends_with(".put")
            || name.ends_with(".patch")
            || name.ends_with(".delete");
        if !is_http_client {
            return;
        }
        let line = module.line_for_offset(call.range.start().to_usize());
        if module.is_rule_suppressed(line, "security/http-verify-false") {
            return;
        }
        issues.push(Issue {
            check: "security/http-verify-false",
            severity: "error",
            category: "Security",
            line,
            path: module.rel_path.to_string(),
            message: "HTTP client disables TLS certificate verification with verify=False",
            help: "Keep certificate verification enabled. Install the correct CA bundle instead of bypassing TLS validation.",
        });
    });
    issues
}

pub(crate) fn collect_insecure_cookie_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if !module.source.contains("set_cookie(") {
        return Vec::new();
    }

    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        let Some(name) = call_name(&call.func) else {
            return;
        };
        if !name.ends_with("set_cookie") {
            return;
        }
        let line = module.line_for_offset(call.range.start().to_usize());
        if module.is_rule_suppressed(line, "security/insecure-cookie") {
            return;
        }

        let has_secure = call.keywords.iter().any(|kw| {
            kw.arg.as_deref() == Some("secure")
                && matches!(&kw.value, Expr::Constant(value) if matches!(value.value, ast::Constant::Bool(true)))
        });
        let has_httponly = call.keywords.iter().any(|kw| {
            kw.arg.as_deref() == Some("httponly")
                && matches!(&kw.value, Expr::Constant(value) if matches!(value.value, ast::Constant::Bool(true)))
        });
        let has_samesite = call.keywords.iter().any(|kw| {
            kw.arg.as_deref() == Some("samesite")
                && matches!(&kw.value, Expr::Constant(value) if matches!(&value.value, ast::Constant::Str(raw) if !raw.trim().is_empty()))
        });
        if has_secure && has_httponly && has_samesite {
            return;
        }

        issues.push(Issue {
            check: "security/insecure-cookie",
            severity: "warning",
            category: "Security",
            line,
            path: module.rel_path.to_string(),
            message: "Response cookie is missing secure, httponly, or samesite hardening",
            help: "Set secure=True, httponly=True, and an explicit samesite value unless this is a deliberate non-session cookie.",
        });
    });
    issues
}

pub(crate) fn collect_exception_string_response_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if !module.source.contains("except") || !module.source.contains("str(") {
        return Vec::new();
    }

    fn expr_leaks_exception(expr: &Expr, exc_name: &str) -> bool {
        match expr {
            Expr::Call(call) => {
                matches!(&*call.func, Expr::Name(name) if name.id.as_str() == "str")
                    && call
                        .args
                        .first()
                        .is_some_and(|arg| matches!(arg, Expr::Name(name) if name.id.as_str() == exc_name))
            }
            Expr::JoinedStr(joined) => joined.values.iter().any(|value| {
                matches!(
                    value,
                    Expr::FormattedValue(formatted)
                        if matches!(&*formatted.value, Expr::Name(name) if name.id.as_str() == exc_name)
                )
            }),
            _ => false,
        }
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
            let Some(exc_name) = handler.name.as_deref() else {
                continue;
            };
            walk_suite_exprs(&handler.body, &mut |expr| {
                let Expr::Call(call) = expr else { return };
                let Some(name) = call_name(&call.func) else {
                    return;
                };
                let response_like = name.ends_with("Response")
                    || name.ends_with("Event")
                    || name.ends_with("Error")
                    || name.ends_with("HTTPException");
                if !response_like {
                    return;
                }
                let leaks = call
                    .args
                    .iter()
                    .any(|arg| expr_leaks_exception(arg, exc_name))
                    || call.keywords.iter().any(|kw| {
                        matches!(
                            kw.arg.as_deref(),
                            Some("detail" | "message" | "error" | "content" | "text")
                        ) && expr_leaks_exception(&kw.value, exc_name)
                    });
                if !leaks {
                    return;
                }
                let line = module.line_for_offset(call.range.start().to_usize());
                if seen_lines.contains(&line)
                    || module.is_rule_suppressed(line, "security/exception-string-response")
                {
                    return;
                }
                seen_lines.insert(line);
                issues.push(Issue {
                    check: "security/exception-string-response",
                    severity: "warning",
                    category: "Security",
                    line,
                    path: module.rel_path.to_string(),
                    message: "Exception string is exposed through a response or event payload",
                    help: "Log the exception with traceback and return a generic public message.",
                });
            });
        }
    });
    issues
}

pub(crate) fn collect_jwt_insecure_decode_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if !module.source.contains("decode") || !module.source.contains("jwt") {
        return Vec::new();
    }

    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        let Some(name) = call_name(&call.func) else {
            return;
        };
        if !name.ends_with("jwt.decode") && name != "decode" {
            return;
        }

        let has_algorithms = call
            .keywords
            .iter()
            .any(|kw| kw.arg.as_deref() == Some("algorithms"));
        let disables_signature = call.keywords.iter().any(|kw| {
            kw.arg.as_deref() == Some("options")
                && matches!(
                    &kw.value,
                    Expr::Dict(dict)
                        if dict.keys.iter().zip(dict.values.iter()).any(|(key, value)| {
                            matches!(
                                (key, value),
                                (
                                    Some(Expr::Constant(key_const)),
                                    Expr::Constant(value_const)
                                ) if matches!(&key_const.value, ast::Constant::Str(raw) if raw == "verify_signature")
                                    && matches!(value_const.value, ast::Constant::Bool(false))
                            )
                        })
                )
        });
        if has_algorithms && !disables_signature {
            return;
        }

        let line = module.line_for_offset(call.range.start().to_usize());
        if module.is_rule_suppressed(line, "security/jwt-insecure-decode") {
            return;
        }
        let (message, help) = if disables_signature {
            (
                "jwt.decode() disables signature verification",
                "Do not use options={'verify_signature': False} outside tightly isolated tooling.",
            )
        } else {
            (
                "jwt.decode() does not pin allowed algorithms",
                "Pass algorithms=[...] explicitly so tokens cannot choose or confuse the verification algorithm.",
            )
        };
        issues.push(Issue {
            check: "security/jwt-insecure-decode",
            severity: "error",
            category: "Security",
            line,
            path: module.rel_path.to_string(),
            message,
            help,
        });
    });
    issues
}

pub(crate) fn collect_debug_enabled_issues(module: &ModuleIndex, suite: &ast::Suite) -> Vec<Issue> {
    if module.rel_path.contains("tests/")
        || module.rel_path.contains("scripts/")
        || (!module.source.contains("debug=True") && !module.source.contains("reload=True"))
    {
        return Vec::new();
    }

    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        let Some(name) = call_name(&call.func) else {
            return;
        };
        let is_debug_app = keyword_is_true(call, "debug")
            && (name == "FastAPI" || name.ends_with(".FastAPI") || name.ends_with(".run"));
        let is_reload_server =
            keyword_is_true(call, "reload") && (name == "uvicorn.run" || name.ends_with(".run"));
        if !is_debug_app && !is_reload_server {
            return;
        }
        let line = module.line_for_offset(call.range.start().to_usize());
        if module.is_rule_suppressed(line, "security/debug-enabled") {
            return;
        }
        issues.push(Issue {
            check: "security/debug-enabled",
            severity: "error",
            category: "Security",
            line,
            path: module.rel_path.to_string(),
            message: "Debug or reload mode is enabled in application code",
            help: "Keep debug=True and reload=True in local entrypoints only, never in importable production modules.",
        });
    });
    issues
}

pub(crate) fn collect_cors_wildcard_credentials_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
) -> Vec<Issue> {
    if !module.source.contains("allow_credentials") || !module.source.contains("allow_origins") {
        return Vec::new();
    }

    let mut issues = Vec::new();
    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        let Some(name) = call_name(&call.func) else {
            return;
        };
        let is_cors_call = name.ends_with("CORSMiddleware")
            || (name.ends_with("add_middleware")
                && call.args.first().is_some_and(|arg| {
                    matches!(arg, Expr::Name(name) if name.id.as_str() == "CORSMiddleware")
                        || matches!(arg, Expr::Attribute(attr) if attr.attr.as_str() == "CORSMiddleware")
                }));
        if !is_cors_call || !keyword_is_true(call, "allow_credentials") {
            return;
        }
        let wildcard_origin = keyword_list_contains_str(call, "allow_origins", "*")
            || keyword_str_equals(call, "allow_origin_regex", ".*");
        if !wildcard_origin {
            return;
        }
        let line = module.line_for_offset(call.range.start().to_usize());
        if module.is_rule_suppressed(line, "security/cors-wildcard-credentials") {
            return;
        }
        issues.push(Issue {
            check: "security/cors-wildcard-credentials",
            severity: "error",
            category: "Security",
            line,
            path: module.rel_path.to_string(),
            message: "CORS allows credentials with a wildcard origin",
            help: "When credentials are allowed, use explicit trusted origins. Wildcards and credentialed browser requests do not belong together.",
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
