use std::collections::HashSet;

use fastapi_doctor_core::ast_helpers::{
    walk_expr_tree, walk_suite_exprs, walk_suite_stmts, FunctionIndex,
};
use fastapi_doctor_core::{
    issue, parse_suite, Config, Issue, ModuleIndex, ModuleRecord, RouteRecord,
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
        Stmt::FunctionDef(node) => {
            if node.decorator_list.iter().any(decorator_is_startup_event) {
                has_startup_hook = true;
            }
        }
        Stmt::AsyncFunctionDef(node) => {
            if node.decorator_list.iter().any(decorator_is_startup_event) {
                has_startup_hook = true;
            }
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
            let is_validationish = (lower.contains("validate")
                || lower.contains("verify")
                || lower.starts_with("check"))
                && (lower.contains("config")
                    || lower.contains("setting")
                    || lower.contains("env")
                    || lower.contains("startup"));

            if is_validationish
                || call
                    .args
                    .iter()
                    .any(|arg| expr_mentions_config(arg, &config_names))
                || call
                    .keywords
                    .iter()
                    .any(|kw| expr_mentions_config(&kw.value, &config_names))
            {
                if lower.contains("validate")
                    || lower.contains("verify")
                    || lower.starts_with("check")
                {
                    has_validation_call = true;
                }
            }
        }
    });

    has_lifespan || has_config_usage || has_validation_call
}

#[derive(Clone, Default)]
pub struct RuleSelection {
    pub giant_function: bool,
    pub giant_route_handler: bool,
    pub large_function: bool,
    pub deep_nesting: bool,
    pub async_without_await: bool,
    pub import_bloat: bool,
    pub print_in_production: bool,
    pub star_import: bool,
    pub direct_env_access: bool,
    pub asyncio_run_in_async: bool,
    pub sync_io_in_async: bool,
    pub misused_async_constructs: bool,
    pub avoid_os_path: bool,
    pub deprecated_typing_imports: bool,
    pub mutable_default_arg: bool,
    pub import_time_default_call: bool,
    pub naive_datetime: bool,
    pub return_in_finally: bool,
    pub threading_lock_in_async: bool,
    pub unreachable_code: bool,
    pub assert_in_production: bool,
    pub cors_wildcard: bool,
    pub exception_detail_leak: bool,
    pub subprocess_shell_true: bool,
    pub unsafe_yaml_load: bool,
    pub weak_hash_without_flag: bool,
    pub sqlalchemy_pool_pre_ping: bool,
    pub bare_except_pass: bool,
    pub reraise_without_context: bool,
    pub exception_swallowed: bool,
    pub broad_except_no_context: bool,
    pub sql_fstring_interpolation: bool,
    pub hardcoded_secret: bool,
    pub pydantic_secretstr: bool,
    pub sequential_awaits: bool,
    pub regex_in_loop: bool,
    pub n_plus_one_hint: bool,
    pub deprecated_validator: bool,
    pub mutable_model_default: bool,
    pub extra_allow_on_request: bool,
    pub should_be_model: bool,
    pub sensitive_field_type: bool,
    pub normalized_name_collision: bool,
    pub get_with_side_effect: bool,
    pub exposed_mutable_state: bool,
    pub serverless_filesystem_write: bool,
    pub missing_http_timeout: bool,
    pub god_module: bool,
    pub passthrough_function: bool,
    pub hidden_dependency_instantiation: bool,
    pub flag_argument_dispatch: bool,
    pub avoid_sys_exit: bool,
    pub missing_startup_validation: bool,
    pub fat_route_handler: bool,
    pub missing_auth_dep: bool,
    pub forbidden_write_param: bool,
    pub duplicate_route: bool,
    pub missing_response_model: bool,
    pub weak_response_model: bool,
    pub post_status_code: bool,
    pub missing_tags: bool,
    pub missing_docstring: bool,
    pub missing_pagination: bool,
    pub config_alembic_target_metadata: bool,
    pub config_alembic_empty_autogen_revision: bool,
    pub config_sqlalchemy_naming_convention: bool,
    pub env_mutation: bool,
    pub exception_log_without_traceback: bool,
}

impl RuleSelection {
    pub fn from_rules(rules: &[String]) -> Self {
        let mut selection = Self::default();
        for rule in rules.iter().filter_map(|rule| parse_static_rule(rule)) {
            selection.enable(rule);
        }
        selection
    }

    fn enable(&mut self, rule: StaticRule) {
        match rule {
            StaticRule::ArchitectureGiantFunction => self.giant_function = true,
            StaticRule::ArchitectureGiantRouteHandler => self.giant_route_handler = true,
            StaticRule::ArchitectureLargeFunction => self.large_function = true,
            StaticRule::ArchitectureDeepNesting => self.deep_nesting = true,
            StaticRule::ArchitectureAsyncWithoutAwait => self.async_without_await = true,
            StaticRule::ArchitectureImportBloat => self.import_bloat = true,
            StaticRule::ArchitecturePrintInProduction => self.print_in_production = true,
            StaticRule::ArchitectureStarImport => self.star_import = true,
            StaticRule::ArchitectureGodModule => self.god_module = true,
            StaticRule::ArchitecturePassthroughFunction => self.passthrough_function = true,
            StaticRule::ArchitectureHiddenDependencyInstantiation => {
                self.hidden_dependency_instantiation = true
            }
            StaticRule::ArchitectureFlagArgumentDispatch => self.flag_argument_dispatch = true,
            StaticRule::ArchitectureAvoidSysExit => self.avoid_sys_exit = true,
            StaticRule::ArchitectureMissingStartupValidation => {
                self.missing_startup_validation = true
            }
            StaticRule::ArchitectureFatRouteHandler => self.fat_route_handler = true,
            StaticRule::SecurityMissingAuthDep => self.missing_auth_dep = true,
            StaticRule::SecurityForbiddenWriteParam => self.forbidden_write_param = true,
            StaticRule::CorrectnessDuplicateRoute => self.duplicate_route = true,
            StaticRule::CorrectnessMissingResponseModel => self.missing_response_model = true,
            StaticRule::CorrectnessWeakResponseModel => self.weak_response_model = true,
            StaticRule::CorrectnessPostStatusCode => self.post_status_code = true,
            StaticRule::ApiSurfaceMissingTags => self.missing_tags = true,
            StaticRule::ApiSurfaceMissingDocstring => self.missing_docstring = true,
            StaticRule::ApiSurfaceMissingPagination => self.missing_pagination = true,
            StaticRule::ConfigDirectEnvAccess => self.direct_env_access = true,
            StaticRule::ConfigEnvMutation => self.env_mutation = true,
            StaticRule::ConfigAlembicTargetMetadata => self.config_alembic_target_metadata = true,
            StaticRule::ConfigAlembicEmptyAutogenRevision => {
                self.config_alembic_empty_autogen_revision = true
            }
            StaticRule::ConfigSqlalchemyNamingConvention => {
                self.config_sqlalchemy_naming_convention = true
            }
            StaticRule::CorrectnessAsyncioRunInAsync => self.asyncio_run_in_async = true,
            StaticRule::CorrectnessSyncIoInAsync => self.sync_io_in_async = true,
            StaticRule::CorrectnessMisusedAsyncConstructs => self.misused_async_constructs = true,
            StaticRule::CorrectnessAvoidOsPath => self.avoid_os_path = true,
            StaticRule::CorrectnessDeprecatedTypingImports => self.deprecated_typing_imports = true,
            StaticRule::CorrectnessMutableDefaultArg => self.mutable_default_arg = true,
            StaticRule::CorrectnessImportTimeDefaultCall => self.import_time_default_call = true,
            StaticRule::CorrectnessNaiveDatetime => self.naive_datetime = true,
            StaticRule::CorrectnessReturnInFinally => self.return_in_finally = true,
            StaticRule::CorrectnessThreadingLockInAsync => self.threading_lock_in_async = true,
            StaticRule::CorrectnessUnreachableCode => self.unreachable_code = true,
            StaticRule::CorrectnessGetWithSideEffect => self.get_with_side_effect = true,
            StaticRule::CorrectnessExposedMutableState => self.exposed_mutable_state = true,
            StaticRule::CorrectnessServerlessFilesystemWrite => {
                self.serverless_filesystem_write = true
            }
            StaticRule::CorrectnessMissingHttpTimeout => self.missing_http_timeout = true,
            StaticRule::PerformanceSequentialAwaits => self.sequential_awaits = true,
            StaticRule::PerformanceRegexInLoop => self.regex_in_loop = true,
            StaticRule::PerformanceNPlusOneHint => self.n_plus_one_hint = true,
            StaticRule::PydanticDeprecatedValidator => self.deprecated_validator = true,
            StaticRule::PydanticMutableDefault => self.mutable_model_default = true,
            StaticRule::PydanticExtraAllowOnRequest => self.extra_allow_on_request = true,
            StaticRule::PydanticShouldBeModel => self.should_be_model = true,
            StaticRule::PydanticSensitiveFieldType => self.sensitive_field_type = true,
            StaticRule::PydanticNormalizedNameCollision => self.normalized_name_collision = true,
            StaticRule::SecurityAssertInProduction => self.assert_in_production = true,
            StaticRule::SecurityCorsWildcard => self.cors_wildcard = true,
            StaticRule::SecurityExceptionDetailLeak => self.exception_detail_leak = true,
            StaticRule::SecuritySubprocessShellTrue => self.subprocess_shell_true = true,
            StaticRule::SecurityUnsafeYamlLoad => self.unsafe_yaml_load = true,
            StaticRule::SecurityWeakHashWithoutFlag => self.weak_hash_without_flag = true,
            StaticRule::SecuritySqlFstringInterpolation => self.sql_fstring_interpolation = true,
            StaticRule::SecurityHardcodedSecret => self.hardcoded_secret = true,
            StaticRule::SecurityPydanticSecretStr => self.pydantic_secretstr = true,
            StaticRule::ResilienceSqlalchemyPoolPrePing => self.sqlalchemy_pool_pre_ping = true,
            StaticRule::ResilienceBareExceptPass => self.bare_except_pass = true,
            StaticRule::ResilienceReraiseWithoutContext => self.reraise_without_context = true,
            StaticRule::ResilienceExceptionSwallowed => self.exception_swallowed = true,
            StaticRule::ResilienceBroadExceptNoContext => self.broad_except_no_context = true,
            StaticRule::ResilienceExceptionLogWithoutTraceback => {
                self.exception_log_without_traceback = true
            }
        }
    }

    fn any_ast_rules(&self) -> bool {
        self.giant_function
            || self.giant_route_handler
            || self.large_function
            || self.deep_nesting
            || self.async_without_await
            || self.print_in_production
            || self.asyncio_run_in_async
            || self.sync_io_in_async
            || self.misused_async_constructs
            || self.mutable_default_arg
            || self.import_time_default_call
            || self.return_in_finally
            || self.threading_lock_in_async
            || self.unreachable_code
            || self.bare_except_pass
            || self.reraise_without_context
            || self.exception_swallowed
            || self.broad_except_no_context
            || self.exception_log_without_traceback
            || self.sql_fstring_interpolation
            || self.hardcoded_secret
            || self.pydantic_secretstr
            || self.exception_detail_leak
            || self.sequential_awaits
            || self.regex_in_loop
            || self.n_plus_one_hint
            || self.mutable_model_default
            || self.should_be_model
            || self.sensitive_field_type
            || self.normalized_name_collision
            || self.get_with_side_effect
            || self.exposed_mutable_state
            || self.serverless_filesystem_write
            || self.missing_http_timeout
            || self.passthrough_function
            || self.hidden_dependency_instantiation
            || self.flag_argument_dispatch
            || self.avoid_sys_exit
            || self.missing_startup_validation
            || self.fat_route_handler
    }

    fn any_line_rules(&self) -> bool {
        self.star_import
            || self.direct_env_access
            || self.env_mutation
            || self.avoid_os_path
            || self.deprecated_typing_imports
            || self.naive_datetime
            || self.assert_in_production
            || self.cors_wildcard
            || self.subprocess_shell_true
            || self.unsafe_yaml_load
            || self.weak_hash_without_flag
            || self.sqlalchemy_pool_pre_ping
            || self.deprecated_validator
            || self.extra_allow_on_request
            || self.missing_startup_validation
    }

    pub fn any_route_rules(&self) -> bool {
        self.missing_auth_dep
            || self.forbidden_write_param
            || self.duplicate_route
            || self.missing_response_model
            || self.weak_response_model
            || self.post_status_code
            || self.missing_tags
            || self.missing_docstring
            || self.missing_pagination
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

    if (rules.giant_function || rules.giant_route_handler || rules.large_function)
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

    if rules.deep_nesting && config.deep_nesting_threshold > 0 {
        issues.extend(architecture::collect_deep_nesting_issues(
            module,
            suite,
            config.deep_nesting_threshold,
        ));
    }

    if rules.asyncio_run_in_async {
        issues.extend(correctness::collect_asyncio_run_in_async_issues(
            module, suite,
        ));
    }
    if rules.threading_lock_in_async {
        issues.extend(correctness::collect_threading_lock_in_async_issues(
            module, suite,
        ));
    }
    if rules.mutable_default_arg {
        issues.extend(correctness::collect_mutable_default_arg_issues(
            module, suite,
        ));
    }
    if rules.import_time_default_call {
        issues.extend(correctness::collect_import_time_default_call_issues(
            module, suite,
        ));
    }
    if rules.return_in_finally {
        issues.extend(correctness::collect_return_in_finally_issues(module, suite));
    }
    if rules.unreachable_code {
        issues.extend(correctness::collect_unreachable_code_issues(module, suite));
    }

    if rules.bare_except_pass
        || rules.reraise_without_context
        || rules.exception_swallowed
        || rules.broad_except_no_context
        || rules.exception_log_without_traceback
    {
        issues.extend(resilience::collect_resilience_issues(module, suite, rules));
    }
    if rules.sql_fstring_interpolation {
        issues.extend(security::collect_sql_fstring_issues(module, suite));
    }
    if rules.hardcoded_secret {
        issues.extend(security::collect_hardcoded_secret_issues(module, suite));
    }
    if rules.pydantic_secretstr
        || rules.sensitive_field_type
        || rules.mutable_model_default
        || rules.should_be_model
        || rules.normalized_name_collision
    {
        issues.extend(pydantic::collect_pydantic_issues(
            module, suite, rules, config,
        ));
    }
    if rules.avoid_sys_exit {
        issues.extend(architecture::collect_avoid_sys_exit_issues(module, suite));
    }
    if rules.serverless_filesystem_write {
        issues.extend(correctness::collect_serverless_filesystem_write_issues(
            module, suite,
        ));
    }
    if rules.missing_http_timeout {
        issues.extend(correctness::collect_missing_http_timeout_issues(
            module, suite,
        ));
    }
    if rules.regex_in_loop {
        issues.extend(performance::collect_regex_in_loop_issues(module, suite));
    }
    if rules.n_plus_one_hint {
        issues.extend(performance::collect_n_plus_one_hint_issues(module, suite));
    }
    if rules.get_with_side_effect {
        issues.extend(correctness::collect_get_with_side_effect_issues(
            module, suite,
        ));
    }
    if rules.exposed_mutable_state {
        issues.extend(correctness::collect_exposed_mutable_state_issues(
            module, suite,
        ));
    }
    if rules.fat_route_handler {
        issues.extend(architecture::collect_fat_route_handler_issues(
            module, suite, config,
        ));
    }
    if rules.passthrough_function {
        issues.extend(architecture::collect_passthrough_function_issues(
            module, suite,
        ));
    }
    if rules.hidden_dependency_instantiation {
        issues.extend(architecture::collect_hidden_dependency_instantiation_issues(module, suite));
    }
    if rules.flag_argument_dispatch {
        issues.extend(architecture::collect_flag_argument_dispatch_issues(
            module, suite,
        ));
    }
    if rules.sequential_awaits {
        issues.extend(performance::collect_sequential_awaits_issues(module, suite));
    }
    if rules.print_in_production {
        issues.extend(architecture::collect_print_in_production_issues(
            module, suite,
        ));
    }
    if rules.exception_detail_leak {
        issues.extend(security::collect_exception_detail_leak_issues(
            module, suite,
        ));
    }

    if rules.async_without_await || rules.sync_io_in_async || rules.misused_async_constructs {
        let function_index = FunctionIndex::from_suite(module, suite);

        if rules.async_without_await {
            issues.extend(architecture::collect_async_without_await_issues(
                module,
                &function_index,
            ));
        }
        if rules.sync_io_in_async {
            issues.extend(correctness::collect_sync_io_in_async_issues(
                module,
                &function_index,
            ));
        }
        if rules.misused_async_constructs {
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

    if rules.import_bloat
        && config.import_bloat_threshold > 0
        && module.file_name.as_deref() != Some("__init__.py")
        && module.file_name.as_deref() != Some("main.py")
        && !module.has_noqa_architecture
        && module.import_count > config.import_bloat_threshold
    {
        issues.push(Issue {
            check: "architecture/import-bloat",
            severity: "warning",
            category: "Architecture",
            line: 0,
            path: module.rel_path.to_string(),
            message: Box::leak(
                format!(
                    "File has {} imports (>{}) — consider decomposing",
                    module.import_count, config.import_bloat_threshold
                )
                .into_boxed_str(),
            ),
            help: "Use TYPE_CHECKING guards for type-only imports, lazy-import heavy libraries, or split the module.",
        });
    }

    if rules.god_module
        && config.god_module_threshold > 0
        && !module.has_noqa_architecture
        && module.lines.len() > config.god_module_threshold
    {
        issues.push(Issue {
            check: "architecture/god-module",
            severity: "warning",
            category: "Architecture",
            line: 0,
            path: module.rel_path.to_string(),
            message: Box::leak(
                format!(
                    "File is {} lines (>{}) — decompose into focused modules",
                    module.lines.len(),
                    config.god_module_threshold
                )
                .into_boxed_str(),
            ),
            help: "Extract cohesive groups of functions into separate modules. Each module should have one reason to change.",
        });
    }

    if !rules.any_line_rules() {
        return issues;
    }

    let allow_star_import = rules.star_import && module.file_name.as_deref() != Some("__init__.py");
    let allow_direct_env =
        rules.direct_env_access && module.has_path_part(&["routers", "services", "interfaces"]);
    let allow_env_mutation = rules.env_mutation
        && module.file_name.as_deref() != Some("main.py")
        && module.file_name.as_deref() != Some("__main__.py")
        && module.file_name.as_deref() != Some("cli.py")
        && !module.rel_path.contains("scripts/");
    let allow_assert = rules.assert_in_production && !should_skip_assert(module.rel_path);
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

        if rules.cors_wildcard {
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
                check: "architecture/star-import",
                severity: "warning",
                category: "Architecture",
                line: line.number,
                path: module.rel_path.to_string(),
                message: Box::leak(
                    format!(
                        "from {} import * — pollutes namespace and breaks static analysis",
                        module_name
                    )
                    .into_boxed_str(),
                ),
                help: "Import specific names: from module import Name1, Name2",
            });
        }

        if rules.subprocess_shell_true {
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

        if rules.unsafe_yaml_load {
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

        if rules.avoid_os_path {
            if let Some(rest) = line.compact.split("os.path.").nth(1) {
                let attr: String = rest
                    .chars()
                    .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                    .collect();
                if !attr.is_empty() {
                    issues.push(Issue {
                        check: "correctness/avoid-os-path",
                        severity: "warning",
                        category: "Correctness",
                        line: line.number,
                        path: module.rel_path.to_string(),
                        message: Box::leak(
                            format!("os.path.{} usage detected — prefer pathlib.Path", attr)
                                .into_boxed_str(),
                        ),
                        help: "pathlib offers a safer, more robust object-oriented API for paths.",
                    });
                }
            }
        }

        if rules.deprecated_typing_imports
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
                        check: "correctness/deprecated-typing-imports",
                        severity: "warning",
                        category: "Correctness",
                        line: line.number,
                        path: module.rel_path.to_string(),
                        message: Box::leak(
                            format!("Deprecated typing imports: {} — use builtins", found.join(", "))
                                .into_boxed_str(),
                        ),
                        help: "Use list, dict, tuple, set, X | None directly. Add 'from __future__ import annotations' for 3.7+ compat.",
                    });
                }
            }
        }

        if rules.naive_datetime {
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

        if rules.weak_hash_without_flag {
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

        if rules.sqlalchemy_pool_pre_ping {
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

        if rules.deprecated_validator
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

        if rules.extra_allow_on_request
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

        if rules.missing_startup_validation
            && line.number == 1
            && suite.is_some_and(|parsed_suite| is_startup_entrypoint_module(module, parsed_suite))
        {
            if suite.is_some_and(|parsed_suite| !has_startup_validation_signal(parsed_suite)) {
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
    }

    issues
}

fn should_skip_assert(path: &str) -> bool {
    path.contains("tests/") || path.contains("alembic/")
}
