use std::collections::HashSet;

use fastapi_doctor_core::ast_helpers::{
    walk_expr_tree, walk_suite_exprs, walk_suite_stmts, FunctionIndex,
};
use fastapi_doctor_core::{
    analyze_import_surface, issue, parse_suite, Config, Issue, ModuleIndex, ModuleRecord,
    RouteRecord,
};
use rustpython_parser::ast::{self, Expr, Stmt};

use crate::architecture;
use crate::configuration;
use crate::correctness;
use crate::performance;
use crate::pydantic;
use crate::registry::StaticRule;
use crate::resilience;
use crate::routes;
use crate::rule_selector::parse_static_rule;
use crate::security;

fn is_startup_entrypoint_module(module: &ModuleIndex<'_>, suite: &ast::Suite) -> bool {
    if module.file_name.as_deref() != Some("main.py") {
        return false;
    }

    let mut has_fastapi_call = false;
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(node) if node.name.as_str() == "create_app" => return true,
            Stmt::AsyncFunctionDef(node) if node.name.as_str() == "create_app" => return true,
            _ => {}
        }
    }

    walk_suite_exprs(suite, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        if call_callee_name(&call.func).is_some_and(|name| name == "FastAPI") {
            has_fastapi_call = true;
        }
    });

    has_fastapi_call || module.source.contains("FastAPI(") || module.source.contains("FastAPI (")
}

fn call_callee_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(name) => Some(name.id.as_str()),
        Expr::Attribute(attr) => Some(attr.attr.as_str()),
        _ => None,
    }
}

fn is_config_like_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "settings"
        || lower.ends_with("_settings")
        || lower == "config"
        || lower.ends_with("_config")
        || lower == "get_settings"
        || lower == "load_settings"
}

fn is_config_like_module_path(module_path: &str) -> bool {
    module_path.split('.').any(is_config_like_name)
}

fn expr_mentions_config(expr: &Expr, config_names: &HashSet<String>) -> bool {
    let mut found = false;
    walk_expr_tree(expr, &mut |node| {
        if found {
            return;
        }
        match node {
            Expr::Name(name)
                if config_names.contains(name.id.as_str())
                    || is_config_like_name(name.id.as_str()) =>
            {
                found = true;
            }
            Expr::Attribute(attr)
                if matches!(
                    &*attr.value,
                    Expr::Name(base)
                        if config_names.contains(base.id.as_str())
                            || is_config_like_name(base.id.as_str())
                ) =>
            {
                found = true;
            }
            _ => {}
        }
    });
    found
}

fn decorator_is_startup_event(decorator: &Expr) -> bool {
    let Expr::Call(call) = decorator else {
        return false;
    };
    let Expr::Attribute(attr) = &*call.func else {
        return false;
    };
    if attr.attr.as_str() != "on_event" {
        return false;
    }
    matches!(
        call.args.first(),
        Some(Expr::Constant(constant))
            if matches!(&constant.value, ast::Constant::Str(value) if value == "startup")
    )
}

fn has_startup_validation_signal(suite: &ast::Suite) -> bool {
    let mut config_names: HashSet<String> = HashSet::new();
    let mut has_startup_hook = false;

    walk_suite_stmts(suite, &mut |stmt| match stmt {
        Stmt::Import(node) => {
            for alias in &node.names {
                let binding = alias.asname.as_deref().unwrap_or(alias.name.as_str());
                if is_config_like_name(binding) || is_config_like_module_path(alias.name.as_str()) {
                    config_names.insert(binding.to_string());
                }
            }
        }
        Stmt::ImportFrom(node) => {
            let module_is_config = node
                .module
                .as_deref()
                .is_some_and(is_config_like_module_path);
            for alias in &node.names {
                let binding = alias.asname.as_deref().unwrap_or(alias.name.as_str());
                if module_is_config
                    || is_config_like_name(binding)
                    || is_config_like_name(alias.name.as_str())
                {
                    config_names.insert(binding.to_string());
                }
            }
        }
        Stmt::FunctionDef(node) if node.decorator_list.iter().any(decorator_is_startup_event) => {
            has_startup_hook = true;
        }
        Stmt::AsyncFunctionDef(node)
            if node.decorator_list.iter().any(decorator_is_startup_event) =>
        {
            has_startup_hook = true;
        }
        _ => {}
    });

    if has_startup_hook {
        return true;
    }

    let mut has_lifespan = false;
    let mut has_config_usage = false;
    let mut has_validation_call = false;

    walk_suite_exprs(suite, &mut |expr| {
        if has_lifespan && has_config_usage && has_validation_call {
            return;
        }

        let Expr::Call(call) = expr else {
            return;
        };

        if call_callee_name(&call.func).is_some_and(|name| name == "FastAPI")
            && call
                .keywords
                .iter()
                .any(|kw| kw.arg.as_deref() == Some("lifespan"))
        {
            has_lifespan = true;
        }

        if expr_mentions_config(expr, &config_names) {
            has_config_usage = true;
        }

        if let Some(callee_name) = call_callee_name(&call.func) {
            let lower = callee_name.to_ascii_lowercase();
            let has_validation_name = lower.contains("validate")
                || lower.contains("verify")
                || lower.starts_with("check");
            let is_validationish = has_validation_name
                && (lower.contains("config")
                    || lower.contains("setting")
                    || lower.contains("env")
                    || lower.contains("startup"));

            if has_validation_name
                && (is_validationish
                    || call
                        .args
                        .iter()
                        .any(|arg| expr_mentions_config(arg, &config_names))
                    || call
                        .keywords
                        .iter()
                        .any(|kw| expr_mentions_config(&kw.value, &config_names)))
            {
                has_validation_call = true;
            }
        }
    });

    has_lifespan || has_config_usage || has_validation_call
}

#[derive(Clone, Default)]
pub struct RuleSelection {
    enabled: HashSet<StaticRule>,
}

impl RuleSelection {
    pub fn from_rules(rules: &[String]) -> Self {
        Self {
            enabled: rules
                .iter()
                .filter_map(|rule_id| parse_static_rule(rule_id))
                .collect(),
        }
    }

    pub(crate) fn contains(&self, rule: StaticRule) -> bool {
        self.enabled.contains(&rule)
    }

    fn any_ast_rules(&self) -> bool {
        self.enabled.iter().any(|rule| rule.requires_ast())
    }

    fn any_line_rules(&self) -> bool {
        self.enabled.iter().any(|rule| rule.uses_line_scan())
    }

    pub fn any_route_rules(&self) -> bool {
        self.enabled.iter().any(|rule| rule.is_route_rule())
    }
}

pub fn analyze_project_modules(modules: &[ModuleRecord], rules: &RuleSelection) -> Vec<Issue> {
    configuration::collect_project_configuration_issues(modules, rules)
}

pub fn analyze_routes(
    routes: &[RouteRecord],
    rules: &RuleSelection,
    config: &Config,
) -> Vec<Issue> {
    routes::analyze_routes(routes, rules, config)
}

pub fn route_checks_not_evaluated(rules: &RuleSelection, config: &Config) -> Vec<String> {
    routes::route_checks_not_evaluated(rules, config)
}

pub fn analyze_suite(
    module: &ModuleIndex<'_>,
    suite: &ast::Suite,
    rules: &RuleSelection,
    config: &Config,
) -> Vec<Issue> {
    let mut issues = Vec::new();

    if (rules.contains(StaticRule::ArchitectureGiantFunction)
        || rules.contains(StaticRule::ArchitectureGiantRouteHandler)
        || rules.contains(StaticRule::ArchitectureLargeFunction))
        && (config.giant_function_threshold > 0 || config.large_function_threshold > 0)
    {
        let function_index = FunctionIndex::from_suite(module, suite);
        issues.extend(architecture::collect_giant_function_issues(
            module,
            suite,
            &function_index,
            rules,
            config,
        ));
    }

    if rules.contains(StaticRule::ArchitectureDeepNesting) && config.deep_nesting_threshold > 0 {
        issues.extend(architecture::collect_deep_nesting_issues(
            module,
            suite,
            config.deep_nesting_threshold,
        ));
    }

    if rules.contains(StaticRule::CorrectnessAsyncioRunInAsync) {
        issues.extend(correctness::collect_asyncio_run_in_async_issues(
            module, suite,
        ));
    }
    if rules.contains(StaticRule::CorrectnessThreadingLockInAsync) {
        issues.extend(correctness::collect_threading_lock_in_async_issues(
            module, suite,
        ));
    }
    if rules.contains(StaticRule::CorrectnessMutableDefaultArg) {
        issues.extend(correctness::collect_mutable_default_arg_issues(
            module, suite,
        ));
    }
    if rules.contains(StaticRule::CorrectnessImportTimeDefaultCall) {
        issues.extend(correctness::collect_import_time_default_call_issues(
            module, suite,
        ));
    }
    if rules.contains(StaticRule::CorrectnessReturnInFinally) {
        issues.extend(correctness::collect_return_in_finally_issues(module, suite));
    }
    if rules.contains(StaticRule::CorrectnessUnreachableCode) {
        issues.extend(correctness::collect_unreachable_code_issues(module, suite));
    }

    if rules.contains(StaticRule::ResilienceBareExceptPass)
        || rules.contains(StaticRule::ResilienceReraiseWithoutContext)
        || rules.contains(StaticRule::ResilienceExceptionSwallowed)
        || rules.contains(StaticRule::ResilienceBroadExceptNoContext)
        || rules.contains(StaticRule::ResilienceExceptionLogWithoutTraceback)
    {
        issues.extend(resilience::collect_resilience_issues(module, suite, rules));
    }
    if rules.contains(StaticRule::SecuritySqlFstringInterpolation) {
        issues.extend(security::collect_sql_fstring_issues(module, suite));
    }
    if rules.contains(StaticRule::SecuritySqlExecuteFstring) {
        issues.extend(security::collect_sql_execute_fstring_issues(module, suite));
    }
    if rules.contains(StaticRule::SecurityUnsafeEvalExec) {
        issues.extend(security::collect_unsafe_eval_exec_issues(module, suite));
    }
    if rules.contains(StaticRule::SecurityUnsafePickleLoad) {
        issues.extend(security::collect_unsafe_pickle_load_issues(module, suite));
    }
    if rules.contains(StaticRule::SecurityHttpVerifyFalse) {
        issues.extend(security::collect_http_verify_false_issues(module, suite));
    }
    if rules.contains(StaticRule::SecurityInsecureCookie) {
        issues.extend(security::collect_insecure_cookie_issues(module, suite));
    }
    if rules.contains(StaticRule::SecurityExceptionStringResponse) {
        issues.extend(security::collect_exception_string_response_issues(
            module, suite,
        ));
    }
    if rules.contains(StaticRule::SecurityJwtInsecureDecode) {
        issues.extend(security::collect_jwt_insecure_decode_issues(module, suite));
    }
    if rules.contains(StaticRule::SecurityDebugEnabled) {
        issues.extend(security::collect_debug_enabled_issues(module, suite));
    }
    if rules.contains(StaticRule::SecurityCorsWildcardCredentials) {
        issues.extend(security::collect_cors_wildcard_credentials_issues(
            module, suite,
        ));
    }
    if rules.contains(StaticRule::SecurityUnvalidatedRedirect) {
        issues.extend(security::collect_unvalidated_redirect_issues(module, suite));
    }
    if rules.contains(StaticRule::SecurityHardcodedSecret) {
        issues.extend(security::collect_hardcoded_secret_issues(module, suite));
    }
    if rules.contains(StaticRule::SecurityPydanticSecretStr)
        || rules.contains(StaticRule::PydanticSensitiveFieldType)
        || rules.contains(StaticRule::PydanticMutableDefault)
        || rules.contains(StaticRule::PydanticShouldBeModel)
        || rules.contains(StaticRule::PydanticNormalizedNameCollision)
    {
        issues.extend(pydantic::collect_pydantic_issues(
            module, suite, rules, config,
        ));
    }
    if rules.contains(StaticRule::ArchitectureAvoidSysExit) {
        issues.extend(architecture::collect_avoid_sys_exit_issues(module, suite));
    }
    if rules.contains(StaticRule::CorrectnessServerlessFilesystemWrite) {
        issues.extend(correctness::collect_serverless_filesystem_write_issues(
            module, suite,
        ));
    }
    if rules.contains(StaticRule::CorrectnessMissingHttpTimeout) {
        issues.extend(correctness::collect_missing_http_timeout_issues(
            module, suite,
        ));
    }
    if rules.contains(StaticRule::CorrectnessUntrackedBackgroundTask) {
        issues.extend(correctness::collect_untracked_background_task_issues(
            module, suite,
        ));
    }
    if rules.contains(StaticRule::PerformanceRegexInLoop) {
        issues.extend(performance::collect_regex_in_loop_issues(module, suite));
    }
    if rules.contains(StaticRule::PerformanceNPlusOneHint) {
        issues.extend(performance::collect_n_plus_one_hint_issues(module, suite));
    }
    if rules.contains(StaticRule::CorrectnessGetWithSideEffect) {
        issues.extend(correctness::collect_get_with_side_effect_issues(
            module, suite,
        ));
    }
    if rules.contains(StaticRule::CorrectnessExposedMutableState) {
        issues.extend(correctness::collect_exposed_mutable_state_issues(
            module, suite,
        ));
    }
    if rules.contains(StaticRule::ArchitectureFatRouteHandler) {
        issues.extend(architecture::collect_fat_route_handler_issues(
            module, suite, config,
        ));
    }
    if rules.contains(StaticRule::ArchitectureHttpExceptionInService) {
        issues.extend(architecture::collect_httpexception_in_service_issues(
            module, suite,
        ));
    }
    if rules.contains(StaticRule::ArchitectureServicePositionalArgs) {
        issues.extend(architecture::collect_service_positional_args_issues(
            module, suite,
        ));
    }
    if rules.contains(StaticRule::ArchitecturePassthroughFunction) {
        issues.extend(architecture::collect_passthrough_function_issues(
            module, suite,
        ));
    }
    if rules.contains(StaticRule::ArchitectureHiddenDependencyInstantiation) {
        issues.extend(architecture::collect_hidden_dependency_instantiation_issues(module, suite));
    }
    if rules.contains(StaticRule::ArchitectureFlagArgumentDispatch) {
        issues.extend(architecture::collect_flag_argument_dispatch_issues(
            module, suite,
        ));
    }
    if rules.contains(StaticRule::PerformanceSequentialAwaits) {
        issues.extend(performance::collect_sequential_awaits_issues(module, suite));
    }
    if rules.contains(StaticRule::ArchitecturePrintInProduction) {
        issues.extend(architecture::collect_print_in_production_issues(
            module, suite,
        ));
    }
    if rules.contains(StaticRule::SecurityExceptionDetailLeak) {
        issues.extend(security::collect_exception_detail_leak_issues(
            module, suite,
        ));
    }

    if rules.contains(StaticRule::ArchitectureAsyncWithoutAwait)
        || rules.contains(StaticRule::CorrectnessSyncIoInAsync)
        || rules.contains(StaticRule::CorrectnessMisusedAsyncConstructs)
    {
        let function_index = FunctionIndex::from_suite(module, suite);

        if rules.contains(StaticRule::ArchitectureAsyncWithoutAwait) {
            issues.extend(architecture::collect_async_without_await_issues(
                module,
                &function_index,
            ));
        }
        if rules.contains(StaticRule::CorrectnessSyncIoInAsync) {
            issues.extend(correctness::collect_sync_io_in_async_issues(
                module,
                &function_index,
            ));
        }
        if rules.contains(StaticRule::CorrectnessMisusedAsyncConstructs) {
            issues.extend(correctness::collect_misused_async_construct_issues(
                module,
                &function_index,
            ));
        }
    }

    issues
}

pub fn analyze_module(
    module: &ModuleRecord,
    rules: &RuleSelection,
    config: &Config,
) -> Result<Vec<Issue>, String> {
    let index = ModuleIndex::new(module);
    let suite = if rules.any_ast_rules() {
        parse_suite(module)
    } else {
        None
    };
    Ok(analyze_module_with_suite(
        &index,
        suite.as_ref(),
        rules,
        config,
    ))
}

pub fn analyze_module_with_suite(
    module: &ModuleIndex<'_>,
    suite: Option<&ast::Suite>,
    rules: &RuleSelection,
    config: &Config,
) -> Vec<Issue> {
    let mut issues = Vec::new();

    if let Some(parsed_suite) = suite {
        issues.extend(analyze_suite(module, parsed_suite, rules, config));
    }

    if rules.contains(StaticRule::ArchitectureImportBloat)
        && config.import_bloat_threshold > 0
        && module.file_name.as_deref() != Some("__init__.py")
        && module.file_name.as_deref() != Some("main.py")
        && !module.has_noqa_architecture
    {
        if let Some(parsed_suite) = suite {
            let summary = analyze_import_surface(parsed_suite);
            if let Some(issue) = architecture::collect_import_bloat_issue(
                module,
                &summary,
                config.import_bloat_threshold,
            ) {
                issues.push(issue);
            }
        }
    }

    if rules.contains(StaticRule::ArchitectureGodModule)
        && config.god_module_threshold > 0
        && !module.has_noqa_architecture
        && module.lines.len() > config.god_module_threshold
    {
        issues.push(Issue {
            check: "architecture/god-module".into(),
            severity: "warning",
            category: "Architecture",
            line: 0,
            path: module.rel_path.to_string(),
            message:
                format!(
                    "File is {} lines (>{}) — decompose into focused modules",
                    module.lines.len(),
                    config.god_module_threshold
                )
                .into(),
            help: "Extract cohesive groups of functions into separate modules. Each module should have one reason to change.".into(),
        });
    }

    if !rules.any_line_rules() {
        return issues;
    }

    let allow_star_import = rules.contains(StaticRule::ArchitectureStarImport)
        && module.file_name.as_deref() != Some("__init__.py");
    let allow_direct_env = rules.contains(StaticRule::ConfigDirectEnvAccess)
        && module.has_path_part(&["routers", "services", "interfaces"]);
    let allow_env_mutation = rules.contains(StaticRule::ConfigEnvMutation)
        && module.file_name.as_deref() != Some("main.py")
        && module.file_name.as_deref() != Some("__main__.py")
        && module.file_name.as_deref() != Some("cli.py")
        && !module.rel_path.contains("scripts/");
    let allow_assert = rules.contains(StaticRule::SecurityAssertInProduction)
        && !should_skip_assert(module.rel_path);
    let deprecated_typing = [
        "List",
        "Dict",
        "Tuple",
        "Set",
        "FrozenSet",
        "Type",
        "Optional",
        "Union",
    ];
    for line in &module.lines {
        if allow_assert
            && (line.trimmed_start.starts_with("assert ")
                || line.trimmed_start.starts_with("assert("))
        {
            issues.push(issue(
                "security/assert-in-production",
                "error",
                "Security",
                line.number,
                module.rel_path,
                "assert statement outside tests — use explicit exception raises",
                "Asserts are ignored when Python runs with -O. Raise ValueError or custom exceptions instead. Do not wrap in 'if condition:' without raising, as that silently skips the check.",
            ));
        }

        if rules.contains(StaticRule::ArchitectureSlopComment)
            && !module.has_path_part(&["tests", "test", "vendor", "vendored", "third_party"])
            && !module.is_rule_suppressed(line.number, "architecture/slop-comment")
        {
            let lower = line.raw.to_ascii_lowercase();
            let comment = lower.split('#').nth(1).unwrap_or("");
            let has_marker = comment.contains("todo")
                || comment.contains("fixme")
                || comment.contains("hack")
                || comment.contains("xxx")
                || comment.contains("temporary")
                || comment.contains("workaround")
                || comment.contains("legacy")
                || comment.contains("backward compat")
                || comment.contains("compatibility")
                || comment.contains("fallback")
                || comment.contains("placeholder")
                || comment.contains("stub")
                || comment.contains("remove this")
                || comment.contains("remove these")
                || comment.contains("defensive");
            if has_marker && !line.raw.contains("# noqa") && !line.raw.contains("doctor:ignore") {
                issues.push(issue(
                    "architecture/slop-comment",
                    "warning",
                    "Architecture",
                    line.number,
                    module.rel_path,
                    "Cleanup marker comment left in production code",
                    "Resolve the TODO/legacy/fallback note or suppress it with a reason if it documents an intentional boundary.",
                ));
            }
        }

        if rules.contains(StaticRule::SecurityCorsWildcard) {
            let has_cors = line.compact.contains("CORSMiddleware(")
                || line.compact.contains(".add_middleware(CORSMiddleware");
            let wildcard_origins = line.compact.contains("allow_origins=[\"*\"]")
                || line.compact.contains("allow_origins=['*']");
            if has_cors && wildcard_origins && !line.raw.contains("# noqa") {
                issues.push(issue(
                    "security/cors-wildcard",
                    "warning",
                    "Security",
                    line.number,
                    module.rel_path,
                    "CORSMiddleware with allow_origins=['*'] — any site can call your API",
                    "Specify explicit allowed origins: allow_origins=['https://yourdomain.com']",
                ));
            }
        }

        if allow_star_import
            && line.trimmed.starts_with("from ")
            && line.trimmed.contains(" import *")
            && !line.raw.contains("# noqa")
        {
            let module_name = line
                .trimmed
                .strip_prefix("from ")
                .and_then(|rest| rest.split(" import *").next())
                .unwrap_or("module");
            issues.push(Issue {
                check: "architecture/star-import".into(),
                severity: "warning",
                category: "Architecture",
                line: line.number,
                path: module.rel_path.to_string(),
                message: format!(
                    "from {} import * — pollutes namespace and breaks static analysis",
                    module_name
                )
                .into(),
                help: "Import specific names: from module import Name1, Name2".into(),
            });
        }

        if rules.contains(StaticRule::SecuritySubprocessShellTrue) {
            let has_target = line.compact.contains("subprocess.Popen(")
                || line.compact.contains("subprocess.run(")
                || line.compact.contains("subprocess.call(")
                || line.compact.contains("subprocess.check_call(")
                || line.compact.contains("subprocess.check_output(");
            if has_target && line.compact.contains("shell=True") {
                issues.push(issue(
                    "security/subprocess-shell-true",
                    "error",
                    "Security",
                    line.number,
                    module.rel_path,
                    "subprocess executed with shell=True — potential shell injection",
                    "Pass arguments as a list and remove shell=True to avoid injection risks.",
                ));
            }
        }

        if rules.contains(StaticRule::SecurityUnsafeYamlLoad) {
            let safe_loader = line.compact.contains("Loader=yaml.SafeLoader")
                || line.compact.contains("Loader=yaml.BaseLoader")
                || line.compact.contains("Loader=yaml.CSafeLoader");
            if line.raw.contains("yaml.load(") && !line.raw.contains("nosec") && !safe_loader {
                issues.push(issue(
                    "security/unsafe-yaml-load",
                    "error",
                    "Security",
                    line.number,
                    module.rel_path,
                    "yaml.load() without SafeLoader/BaseLoader allows arbitrary code execution",
                    "Use yaml.safe_load() or pass Loader=yaml.SafeLoader.",
                ));
            }
        }

        if rules.contains(StaticRule::CorrectnessAvoidOsPath) {
            if let Some(rest) = line.compact.split("os.path.").nth(1) {
                let attr: String = rest
                    .chars()
                    .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                    .collect();
                if !attr.is_empty() {
                    issues.push(Issue {
                        check: "correctness/avoid-os-path".into(),
                        severity: "warning",
                        category: "Correctness",
                        line: line.number,
                        path: module.rel_path.to_string(),
                        message: format!("os.path.{} usage detected — prefer pathlib.Path", attr)
                            .into(),
                        help: "pathlib offers a safer, more robust object-oriented API for paths."
                            .into(),
                    });
                }
            }
        }

        if rules.contains(StaticRule::CorrectnessDeprecatedTypingImports)
            && line.trimmed.starts_with("from typing import")
            && !line.raw.contains("# noqa")
        {
            if let Some(imported) = line
                .trimmed
                .strip_prefix("from typing import")
                .map(str::trim)
            {
                let found: Vec<&str> = imported
                    .split(',')
                    .map(|part| part.trim())
                    .filter(|name| deprecated_typing.contains(name))
                    .collect();
                if !found.is_empty() {
                    issues.push(Issue {
                        check: "correctness/deprecated-typing-imports".into(),
                        severity: "warning",
                        category: "Correctness",
                        line: line.number,
                        path: module.rel_path.to_string(),
                        message:
                            format!("Deprecated typing imports: {} — use builtins", found.join(", "))
                                .into(),
                        help: "Use list, dict, tuple, set, X | None directly. Add 'from __future__ import annotations' for 3.7+ compat.".into(),
                    });
                }
            }
        }

        if rules.contains(StaticRule::CorrectnessNaiveDatetime) {
            if line.compact.contains("datetime.utcnow()") {
                issues.push(issue(
                    "correctness/naive-datetime",
                    "warning",
                    "Correctness",
                    line.number,
                    module.rel_path,
                    "datetime.utcnow() is deprecated — use datetime.now(tz=UTC)",
                    "from datetime import UTC; datetime.now(tz=UTC)",
                ));
            } else if line.compact.contains("datetime.now()") {
                issues.push(issue(
                    "correctness/naive-datetime",
                    "warning",
                    "Correctness",
                    line.number,
                    module.rel_path,
                    "datetime.now() without timezone — use datetime.now(tz=UTC)",
                    "from datetime import UTC; datetime.now(tz=UTC)",
                ));
            }
        }

        if allow_direct_env {
            let direct_env_candidate = line.trimmed.contains("os.environ")
                && !line.trimmed.contains("# noqa: direct-env")
                && !line.trimmed.contains("os.environ.setdefault")
                && !line.trimmed.contains("]= ")
                && !line.trimmed.contains("] =");
            if direct_env_candidate {
                let get_pos = line.trimmed.find("os.environ.get(");
                let bracket_pos = line.trimmed.find("os.environ[");
                let reads_bracket = bracket_pos.is_some();
                let reads_get = if let Some(pos) = get_pos {
                    !line.trimmed[pos..].contains(',')
                } else {
                    false
                };
                if reads_bracket || reads_get {
                    issues.push(issue(
                        "config/direct-env-access",
                        "warning",
                        "Config",
                        line.number,
                        module.rel_path,
                        "Direct os.environ access in service/router code — use settings object",
                        "Read env vars in one config/settings module, then inject the typed setting where needed.",
                    ));
                }
            }
        }

        if allow_env_mutation
            && !line.trimmed.contains("# noqa: env-mutation")
            && (line.trimmed.contains("os.environ.setdefault(")
                || line.trimmed.contains("os.putenv(")
                || line.trimmed.contains("os.environ["))
            && (line.trimmed.contains("os.environ.setdefault(")
                || line.trimmed.contains("os.putenv(")
                || line.trimmed.contains("] =")
                || line.trimmed.contains("]="))
        {
            issues.push(issue(
                "config/env-mutation",
                "warning",
                "Config",
                line.number,
                module.rel_path,
                "Process environment mutated outside bootstrap code — move env setup to startup/config entrypoints",
                "Only mutate os.environ in main.py, __main__.py, cli.py, or scripts. Pass values through typed settings elsewhere.",
            ));
        }

        if rules.contains(StaticRule::SecurityWeakHashWithoutFlag) {
            let uses_hash = line.compact.contains("sha1(") || line.compact.contains("md5(");
            let uses_hexdigest = line.compact.contains(".hexdigest()");
            let has_flag = line.compact.contains("usedforsecurity=False");
            if !line.raw.contains("nosec") && uses_hash && uses_hexdigest && !has_flag {
                issues.push(issue(
                    "security/weak-hash-without-flag",
                    "error",
                    "Security",
                    line.number,
                    module.rel_path,
                    "SHA1/MD5 used without usedforsecurity=False",
                    "Add usedforsecurity=False to signal this is not for security purposes.",
                ));
            }
        }

        if rules.contains(StaticRule::ResilienceSqlalchemyPoolPrePing) {
            let is_engine_call =
                line.compact.contains("create_engine(") || line.compact.contains(".create_engine(");
            if is_engine_call && !line.compact.contains("pool_pre_ping=True") {
                issues.push(issue(
                    "resilience/sqlalchemy-pool-pre-ping",
                    "warning",
                    "Resilience",
                    line.number,
                    module.rel_path,
                    "SQLAlchemy engine without pool_pre_ping=True",
                    "Add pool_pre_ping=True to create_engine() to ensure automatic recovery from dropped connections.",
                ));
            }
        }

        if rules.contains(StaticRule::PydanticDeprecatedValidator)
            && line.trimmed.starts_with("@validator(")
            && !line.raw.contains("field_validator")
        {
            issues.push(issue(
                "pydantic/deprecated-validator",
                "error",
                "Pydantic",
                line.number,
                module.rel_path,
                "@validator is deprecated (Pydantic v1) — use @field_validator",
                "Replace @validator('field', pre=True) with @field_validator('field', mode='before').",
            ));
        }

        if rules.contains(StaticRule::PydanticExtraAllowOnRequest)
            && module.has_path_part(&["routers", "interfaces"])
            && (line.raw.contains("extra=\"allow\"") || line.raw.contains("extra='allow'"))
        {
            issues.push(issue(
                "pydantic/extra-allow-on-request",
                "warning",
                "Pydantic",
                line.number,
                module.rel_path,
                "Model in request path uses extra='allow' — accepts arbitrary user input",
                "Use extra='ignore' (drop unknown fields) or extra='forbid' (reject them).",
            ));
        }

        if rules.contains(StaticRule::ArchitectureMissingStartupValidation)
            && line.number == 1
            && suite.is_some_and(|parsed_suite| {
                is_startup_entrypoint_module(module, parsed_suite)
                    && !has_startup_validation_signal(parsed_suite)
            })
        {
            issues.push(issue(
                "architecture/missing-startup-validation",
                "warning",
                "Architecture",
                1,
                module.rel_path,
                "Main app entry point creates the FastAPI app without an evident startup/lifespan validation or settings bootstrap signal",
                "Add a lifespan/startup hook or touch validated settings/config during app bootstrap so startup fails fast when configuration is broken.",
            ));
        }
    }

    issues
}

fn should_skip_assert(path: &str) -> bool {
    path.contains("tests/") || path.contains("alembic/")
}
