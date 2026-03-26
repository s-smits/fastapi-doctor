use rustpython_parser::ast::{self, Expr, Ranged, Stmt};

use crate::{Issue, ModuleIndex, RuleSelection};
use crate::ast_helpers::*;

pub(crate) fn collect_resilience_issues(
    module: &ModuleIndex,
    suite: &ast::Suite,
    rules: &RuleSelection,
) -> Vec<Issue> {
    if !module.source.contains("except") {
        return Vec::new();
    }
    let mut issues = Vec::new();
    walk_suite_stmts(suite, &mut |stmt| {
        let handlers = match stmt {
            Stmt::Try(node) => Some(&node.handlers),
            Stmt::TryStar(node) => Some(&node.handlers),
            _ => None,
        };
        let Some(handlers) = handlers else { return };
        for handler in handlers {
            let ast::ExceptHandler::ExceptHandler(handler) = handler;
            let handler_line = module.line_for_offset(handler.range.start().to_usize());

            // bare-except-pass: except handler body is just `pass` with no comment
            if rules.bare_except_pass
                && handler.body.len() == 1
                && matches!(handler.body[0], Stmt::Pass(_))
            {
                let pass_line = module.line_for_offset(handler.body[0].range().start().to_usize());
                let mut has_comment = false;
                for check_line in handler_line..=pass_line {
                    if check_line > 0
                        && check_line <= module.lines.len()
                        && module.lines[check_line - 1].raw.contains('#')
                    {
                        has_comment = true;
                        break;
                    }
                }
                if !has_comment {
                    issues.push(Issue {
                        check: "resilience/bare-except-pass",
                        severity: "warning",
                        category: "Resilience",
                        line: handler_line,
                        path: module.rel_path.clone(),
                        message: "except: pass silently swallows errors without logging or comment",
                        help: "Add logger.debug/warning or a # comment explaining why it's safe to ignore.",
                    });
                }
            }

            // reraise-without-context
            if rules.reraise_without_context && !handler.body.is_empty() {
                let last_stmt = &handler.body[handler.body.len() - 1];
                let is_bare_raise = matches!(last_stmt, Stmt::Raise(r) if r.exc.is_none());
                let is_identity_raise = if let Stmt::Raise(r) = last_stmt {
                    if let Some(exc) = &r.exc {
                        if let Expr::Name(name) = &**exc {
                            handler.name.as_deref() == Some(name.id.as_str())
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };
                if is_bare_raise || is_identity_raise {
                    let preceding = &handler.body[..handler.body.len() - 1];
                    let has_useful_work = preceding.iter().any(|s| {
                        matches!(
                            s,
                            Stmt::Assign(_)
                                | Stmt::AugAssign(_)
                                | Stmt::AnnAssign(_)
                                | Stmt::If(_)
                                | Stmt::For(_)
                                | Stmt::While(_)
                                | Stmt::With(_)
                                | Stmt::Try(_)
                        ) || matches!(s, Stmt::Expr(e) if matches!(&*e.value, Expr::Call(_)))
                    });
                    if !has_useful_work
                        && !module.is_rule_suppressed(handler_line, "resilience/reraise-without-context")
                    {
                        issues.push(Issue {
                            check: "resilience/reraise-without-context",
                            severity: "warning",
                            category: "Resilience",
                            line: handler_line,
                            path: module.rel_path.clone(),
                            message: "except handler re-raises without adding context — remove the try/except or add info",
                            help: "Either remove the try/except (it's noise) or use `raise NewError(...) from exc`.",
                        });
                    }
                }
            }

            // exception-swallowed and broad-except-no-context: only for `except Exception`
            let is_except_exception = handler.type_.as_ref().is_some_and(|t| {
                matches!(&**t, Expr::Name(n) if n.id.as_str() == "Exception")
            });
            if !is_except_exception {
                continue;
            }
            if module.is_rule_suppressed(handler_line, "resilience/exception-swallowed")
                && module.is_rule_suppressed(handler_line, "resilience/broad-except-no-context")
            {
                continue;
            }

            let body = &handler.body;
            let exc_name = handler.name.as_deref();
            let mut has_logging = false;
            let mut has_raise = false;
            let mut refs_exc = false;
            let mut log_call_without_context: Option<(usize, &str)> = None;

            walk_suite_exprs(body, &mut |expr| {
                if let Expr::Call(call) = expr {
                    if let Expr::Attribute(func) = &*call.func {
                        if let Expr::Name(obj) = &*func.value {
                            let is_log = matches!(obj.id.as_str(), "logger" | "logging" | "log");
                            if is_log {
                                has_logging = true;
                                let has_exc_info = call.keywords.iter().any(|kw| {
                                    kw.arg.as_deref() == Some("exc_info")
                                        && matches!(&kw.value, Expr::Constant(c) if matches!(c.value, ast::Constant::Bool(true)))
                                });
                                if !has_exc_info
                                    && matches!(func.attr.as_str(), "warning" | "warn" | "info" | "debug")
                                    && log_call_without_context.is_none()
                                {
                                    let mut call_refs_exc = false;
                                    if let Some(exc_n) = exc_name {
                                        for arg in &call.args {
                                            if matches!(arg, Expr::Name(n) if n.id.as_str() == exc_n) {
                                                call_refs_exc = true;
                                            }
                                            if let Expr::JoinedStr(fstr) = arg {
                                                for val in &fstr.values {
                                                    if let Expr::FormattedValue(fv) = val {
                                                        if matches!(&*fv.value, Expr::Name(n) if n.id.as_str() == exc_n)
                                                        {
                                                            call_refs_exc = true;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    if !call_refs_exc {
                                        let line =
                                            module.line_for_offset(call.range.start().to_usize());
                                        log_call_without_context =
                                            Some((line, func.attr.as_str()));
                                    }
                                }
                            }
                        }
                    }
                }
            });
            walk_suite_stmts(body, &mut |s| {
                if matches!(s, Stmt::Raise(_)) {
                    has_raise = true;
                }
            });
            if let Some(exc_n) = exc_name {
                walk_suite_exprs(body, &mut |expr| {
                    if matches!(expr, Expr::Name(n) if n.id.as_str() == exc_n) {
                        refs_exc = true;
                    }
                });
            }

            // exception-swallowed: no logging, no raise, and exc unused or just pass/return
            if rules.exception_swallowed
                && !has_logging
                && !has_raise
                && !module.is_rule_suppressed(handler_line, "resilience/exception-swallowed")
            {
                let is_just_pass = body.len() == 1 && matches!(body[0], Stmt::Pass(_));
                let is_just_return = body.len() == 1 && matches!(body[0], Stmt::Return(_));
                let exc_unused = exc_name.is_some() && !refs_exc;
                if is_just_pass || is_just_return || exc_unused {
                    issues.push(Issue {
                        check: "resilience/exception-swallowed",
                        severity: "warning",
                        category: "Resilience",
                        line: handler_line,
                        path: module.rel_path.clone(),
                        message: "except Exception block swallows error without logging or re-raising",
                        help: "Add logger.exception() or logger.warning(..., exc_info=True) to preserve debugging context.",
                    });
                }
            }

            // broad-except-no-context: logging without exc_info
            if rules.broad_except_no_context
                && !has_raise
                && !module.is_rule_suppressed(handler_line, "resilience/broad-except-no-context")
            {
                if let Some((log_line, log_attr)) = log_call_without_context {
                    issues.push(Issue {
                        check: "resilience/broad-except-no-context",
                        severity: "warning",
                        category: "Resilience",
                        line: log_line,
                        path: module.rel_path.clone(),
                        message: Box::leak(
                            format!(
                                "except Exception logs via logger.{}() but discards traceback",
                                log_attr
                            )
                            .into_boxed_str(),
                        ),
                        help: "Add exc_info=True to the logging call or include the exception variable in the message.",
                    });
                }
            }
        }
    });
    issues
}

// ── Security: SQL f-string interpolation ────────────────────────────────

