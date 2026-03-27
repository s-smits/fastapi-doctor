use fastapi_doctor_core::{
    Config, Issue, ModuleIndex, ModuleRecord, issue, parse_suite,
};
use fastapi_doctor_core::ast_helpers::FunctionIndex;
use rustpython_parser::ast;

use crate::architecture;
use crate::configuration;
use crate::correctness;
use crate::performance;
use crate::pydantic;
use crate::registry::StaticRule;
use crate::resilience;
use crate::rule_selector::parse_static_rule;
use crate::security;

#[derive(Clone, Default)]
pub struct RuleSelection {
    pub giant_function: bool,
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
    pub naive_datetime: bool,
    pub return_in_finally: bool,
    pub threading_lock_in_async: bool,
    pub unreachable_code: bool,
    pub heavy_imports: bool,
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
    pub get_with_side_effect: bool,
    pub serverless_filesystem_write: bool,
    pub missing_http_timeout: bool,
    pub god_module: bool,
    pub passthrough_function: bool,
    pub avoid_sys_exit: bool,
    pub engine_pool_pre_ping: bool,
    pub missing_startup_validation: bool,
    pub fat_route_handler: bool,
    pub config_alembic_target_metadata: bool,
    pub config_alembic_empty_autogen_revision: bool,
    pub config_sqlalchemy_naming_convention: bool,
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
            StaticRule::ArchitectureDeepNesting => self.deep_nesting = true,
            StaticRule::ArchitectureAsyncWithoutAwait => self.async_without_await = true,
            StaticRule::ArchitectureImportBloat => self.import_bloat = true,
            StaticRule::ArchitecturePrintInProduction => self.print_in_production = true,
            StaticRule::ArchitectureStarImport => self.star_import = true,
            StaticRule::ArchitectureGodModule => self.god_module = true,
            StaticRule::ArchitecturePassthroughFunction => self.passthrough_function = true,
            StaticRule::ArchitectureAvoidSysExit => self.avoid_sys_exit = true,
            StaticRule::ArchitectureEnginePoolPrePing => self.engine_pool_pre_ping = true,
            StaticRule::ArchitectureMissingStartupValidation => self.missing_startup_validation = true,
            StaticRule::ArchitectureFatRouteHandler => self.fat_route_handler = true,
            StaticRule::ConfigDirectEnvAccess => self.direct_env_access = true,
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
            StaticRule::CorrectnessNaiveDatetime => self.naive_datetime = true,
            StaticRule::CorrectnessReturnInFinally => self.return_in_finally = true,
            StaticRule::CorrectnessThreadingLockInAsync => self.threading_lock_in_async = true,
            StaticRule::CorrectnessUnreachableCode => self.unreachable_code = true,
            StaticRule::CorrectnessGetWithSideEffect => self.get_with_side_effect = true,
            StaticRule::CorrectnessServerlessFilesystemWrite => self.serverless_filesystem_write = true,
            StaticRule::CorrectnessMissingHttpTimeout => self.missing_http_timeout = true,
            StaticRule::PerformanceHeavyImports => self.heavy_imports = true,
            StaticRule::PerformanceSequentialAwaits => self.sequential_awaits = true,
            StaticRule::PerformanceRegexInLoop => self.regex_in_loop = true,
            StaticRule::PerformanceNPlusOneHint => self.n_plus_one_hint = true,
            StaticRule::PydanticDeprecatedValidator => self.deprecated_validator = true,
            StaticRule::PydanticMutableDefault => self.mutable_model_default = true,
            StaticRule::PydanticExtraAllowOnRequest => self.extra_allow_on_request = true,
            StaticRule::PydanticShouldBeModel => self.should_be_model = true,
            StaticRule::PydanticSensitiveFieldType => self.sensitive_field_type = true,
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
        }
    }

    fn any_ast_rules(&self) -> bool {
        self.giant_function
            || self.deep_nesting
            || self.async_without_await
            || self.print_in_production
            || self.asyncio_run_in_async
            || self.sync_io_in_async
            || self.misused_async_constructs
            || self.mutable_default_arg
            || self.return_in_finally
            || self.threading_lock_in_async
            || self.unreachable_code
            || self.bare_except_pass
            || self.reraise_without_context
            || self.exception_swallowed
            || self.broad_except_no_context
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
            || self.get_with_side_effect
            || self.serverless_filesystem_write
            || self.missing_http_timeout
            || self.passthrough_function
            || self.avoid_sys_exit
            || self.engine_pool_pre_ping
            || self.fat_route_handler
    }

    fn any_line_rules(&self) -> bool {
        self.star_import
            || self.direct_env_access
            || self.avoid_os_path
            || self.deprecated_typing_imports
            || self.naive_datetime
            || self.heavy_imports
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
}

pub fn analyze_project_modules(
    modules: &[ModuleRecord],
    rules: &RuleSelection,
) -> Vec<Issue> {
    configuration::collect_project_configuration_issues(modules, rules)
}

pub fn analyze_suite(
    module: &ModuleIndex<'_>,
    suite: &ast::Suite,
    rules: &RuleSelection,
    config: &Config,
) -> Vec<Issue> {
    let mut issues = Vec::new();

    if rules.giant_function
        && (config.giant_function_threshold > 0 || config.large_function_threshold > 0)
    {
        issues.extend(architecture::collect_giant_function_issues(module, suite, config));
    }

    if rules.deep_nesting && config.deep_nesting_threshold > 0 {
        issues.extend(architecture::collect_deep_nesting_issues(
            module,
            suite,
            config.deep_nesting_threshold,
        ));
    }

    if rules.asyncio_run_in_async {
        issues.extend(correctness::collect_asyncio_run_in_async_issues(module, suite));
    }
    if rules.threading_lock_in_async {
        issues.extend(correctness::collect_threading_lock_in_async_issues(module, suite));
    }
    if rules.mutable_default_arg {
        issues.extend(correctness::collect_mutable_default_arg_issues(module, suite));
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
    {
        issues.extend(pydantic::collect_pydantic_issues(module, suite, rules, config));
    }
    if rules.avoid_sys_exit {
        issues.extend(architecture::collect_avoid_sys_exit_issues(module, suite));
    }
    if rules.engine_pool_pre_ping {
        issues.extend(architecture::collect_engine_pool_pre_ping_issues(module, suite));
    }
    if rules.serverless_filesystem_write {
        issues.extend(correctness::collect_serverless_filesystem_write_issues(module, suite));
    }
    if rules.missing_http_timeout {
        issues.extend(correctness::collect_missing_http_timeout_issues(module, suite));
    }
    if rules.regex_in_loop {
        issues.extend(performance::collect_regex_in_loop_issues(module, suite));
    }
    if rules.n_plus_one_hint {
        issues.extend(performance::collect_n_plus_one_hint_issues(module, suite));
    }
    if rules.get_with_side_effect {
        issues.extend(correctness::collect_get_with_side_effect_issues(module, suite));
    }
    if rules.fat_route_handler {
        issues.extend(architecture::collect_fat_route_handler_issues(module, suite, config));
    }
    if rules.passthrough_function {
        issues.extend(architecture::collect_passthrough_function_issues(module, suite));
    }
    if rules.sequential_awaits {
        issues.extend(performance::collect_sequential_awaits_issues(module, suite));
    }
    if rules.print_in_production {
        issues.extend(architecture::collect_print_in_production_issues(
            module,
            suite,
        ));
    }
    if rules.exception_detail_leak {
        issues.extend(security::collect_exception_detail_leak_issues(
            module,
            suite,
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
    Ok(analyze_module_with_suite(&index, suite.as_ref(), rules, config))
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
    let heavy_libs = [
        "agno",
        "openai",
        "pandas",
        "numpy",
        "torch",
        "transformers",
        "playwright",
        "langchain",
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

        if rules.heavy_imports && (line.raw.starts_with("import ") || line.raw.starts_with("from "))
        {
            for lib in heavy_libs {
                let import_prefix = format!("import {}", lib);
                let from_prefix = format!("from {}", lib);
                if line.trimmed_start.starts_with(&import_prefix)
                    || line.trimmed_start.starts_with(&from_prefix)
                {
                    issues.push(Issue {
                        check: "performance/heavy-imports",
                        severity: "warning",
                        category: "Performance",
                        line: line.number,
                        path: module.rel_path.to_string(),
                        message: Box::leak(
                            format!(
                                "Heavy library '{{{}}}' imported at module level — degrades serverless cold-starts",
                                lib
                            )
                            .into_boxed_str(),
                        ),
                        help: "Move the import inside the function or router handler that uses it (lazy loading).",
                    });
                    break;
                }
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
            && module.file_name.as_deref() == Some("main.py")
            && line.number == 1
        {
            let has_validation = module.source.contains("validate_") && module.source.contains("startup")
                || module.source.contains("settings.validate")
                || module.source.contains("check_config")
                || module.source.contains("verify_env");
            if !has_validation {
                issues.push(issue(
                    "architecture/missing-startup-validation",
                    "warning",
                    "Architecture",
                    1,
                    module.rel_path,
                    "Main app entry point missing explicit startup configuration validation",
                    "Add a 'fail-fast' validation step during app startup to verify critical settings.",
                ));
            }
        }
    }

    issues
}

fn should_skip_assert(path: &str) -> bool {
    path.contains("tests/") || path.contains("alembic/")
}
