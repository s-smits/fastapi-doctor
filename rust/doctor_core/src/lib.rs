mod ast_helpers;
mod rules;

use std::path::Path;

#[derive(Clone)]
struct ModuleRecord {
    rel_path: String,
    source: String,
}

#[derive(Clone, Default)]
struct Config {
    import_bloat_threshold: usize,
    giant_function_threshold: usize,
    large_function_threshold: usize,
    deep_nesting_threshold: usize,
    god_module_threshold: usize,
    fat_route_handler_threshold: usize,
    should_be_model_mode: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Issue {
    check: &'static str,
    severity: &'static str,
    category: &'static str,
    line: usize,
    path: String,
    message: &'static str,
    help: &'static str,
}

struct LineRecord {
    number: usize,
    raw: String,
    trimmed: String,
    trimmed_start: String,
    compact: String,
}

struct ModuleIndex {
    rel_path: String,
    source: String,
    lines: Vec<LineRecord>,
    line_starts: Vec<usize>,
    path_parts: Vec<String>,
    file_name: Option<String>,
    import_count: usize,
    has_noqa_architecture: bool,
}

impl ModuleIndex {
    fn new(module: &ModuleRecord) -> Self {
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
            let trimmed_start = raw.trim_start().to_string();
            if trimmed_start.starts_with("import ") || trimmed_start.starts_with("from ") {
                import_count += 1;
            }
            lines.push(LineRecord {
                number: idx + 1,
                raw: raw.to_string(),
                trimmed: raw.trim().to_string(),
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
            rel_path: module.rel_path.clone(),
            source: module.source.clone(),
            lines,
            line_starts,
            path_parts,
            file_name,
            import_count,
            has_noqa_architecture: module.source.contains("# noqa: architecture"),
        }
    }

    fn has_path_part(&self, expected: &[&str]) -> bool {
        self.path_parts
            .iter()
            .any(|part| expected.iter().any(|candidate| part == candidate))
    }

    fn line_for_offset(&self, offset: usize) -> usize {
        self.line_starts.partition_point(|start| *start <= offset)
    }

    fn source_slice(&self, range: rustpython_parser::ast::text_size::TextRange) -> &str {
        let start = range.start().to_usize();
        let end = range.end().to_usize();
        self.source.get(start..end).unwrap_or("")
    }

    fn is_rule_suppressed(&self, line_number: usize, rule_id: &str) -> bool {
        if line_number == 0 || line_number > self.lines.len() {
            return false;
        }
        line_suppresses_rule(&self.lines[line_number - 1].raw, rule_id)
    }
}

fn issue(
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




fn normalized_no_space(line: &str) -> String {
    line.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn selector_matches(rule_id: &str, selector: &str) -> bool {
    let selector = selector.trim();
    if selector.ends_with('*') {
        return rule_id.starts_with(&selector[..selector.len().saturating_sub(1)]);
    }
    if selector.ends_with('/') {
        return rule_id.starts_with(selector);
    }
    rule_id == selector
}

fn line_suppresses_rule(line: &str, rule_id: &str) -> bool {
    if let Some(comment) = line.split('#').nth(1) {
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
                    .map(str::trim)
                    .filter(|code| !code.is_empty())
                {
                    if selector_matches(rule_id, code) {
                        return true;
                    }
                    let alias = match code {
                        "sql-safe" | "security" => Some("security/"),
                        "architecture" => Some("architecture/"),
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

fn should_skip_assert(path: &str) -> bool {
    path.contains("tests/") || path.contains("alembic/")
}

#[derive(Clone, Default)]
struct RuleSelection {
    // Existing rules
    giant_function: bool,
    deep_nesting: bool,
    async_without_await: bool,
    import_bloat: bool,
    print_in_production: bool,
    star_import: bool,
    direct_env_access: bool,
    asyncio_run_in_async: bool,
    sync_io_in_async: bool,
    misused_async_constructs: bool,
    avoid_os_path: bool,
    deprecated_typing_imports: bool,
    mutable_default_arg: bool,
    naive_datetime: bool,
    return_in_finally: bool,
    threading_lock_in_async: bool,
    unreachable_code: bool,
    heavy_imports: bool,
    assert_in_production: bool,
    cors_wildcard: bool,
    exception_detail_leak: bool,
    subprocess_shell_true: bool,
    unsafe_yaml_load: bool,
    weak_hash_without_flag: bool,
    sqlalchemy_pool_pre_ping: bool,
    // New rules — resilience
    bare_except_pass: bool,
    reraise_without_context: bool,
    exception_swallowed: bool,
    broad_except_no_context: bool,
    // New rules — security
    sql_fstring_interpolation: bool,
    hardcoded_secret: bool,
    pydantic_secretstr: bool,
    // New rules — performance
    sequential_awaits: bool,
    regex_in_loop: bool,
    n_plus_one_hint: bool,
    // New rules — pydantic
    deprecated_validator: bool,
    mutable_model_default: bool,
    extra_allow_on_request: bool,
    should_be_model: bool,
    sensitive_field_type: bool,
    // New rules — correctness
    get_with_side_effect: bool,
    serverless_filesystem_write: bool,
    missing_http_timeout: bool,
    // New rules — architecture
    god_module: bool,
    passthrough_function: bool,
    avoid_sys_exit: bool,
    engine_pool_pre_ping: bool,
    missing_startup_validation: bool,
    fat_route_handler: bool,
}

impl RuleSelection {
    fn from_rules(rules: &[String]) -> Self {
        let mut selection = Self::default();
        for rule in rules {
            match rule.as_str() {
                "architecture/giant-function" => selection.giant_function = true,
                "architecture/deep-nesting" => selection.deep_nesting = true,
                "architecture/async-without-await" => selection.async_without_await = true,
                "architecture/import-bloat" => selection.import_bloat = true,
                "architecture/print-in-production" => selection.print_in_production = true,
                "architecture/star-import" => selection.star_import = true,
                "config/direct-env-access" => selection.direct_env_access = true,
                "correctness/asyncio-run-in-async" => selection.asyncio_run_in_async = true,
                "correctness/sync-io-in-async" => selection.sync_io_in_async = true,
                "correctness/misused-async-constructs" => selection.misused_async_constructs = true,
                "correctness/avoid-os-path" => selection.avoid_os_path = true,
                "correctness/deprecated-typing-imports" => {
                    selection.deprecated_typing_imports = true
                }
                "correctness/mutable-default-arg" => selection.mutable_default_arg = true,
                "correctness/naive-datetime" => selection.naive_datetime = true,
                "correctness/return-in-finally" => selection.return_in_finally = true,
                "correctness/threading-lock-in-async" => selection.threading_lock_in_async = true,
                "correctness/unreachable-code" => selection.unreachable_code = true,
                "performance/heavy-imports" => selection.heavy_imports = true,
                "security/assert-in-production" => selection.assert_in_production = true,
                "security/cors-wildcard" => selection.cors_wildcard = true,
                "security/exception-detail-leak" => selection.exception_detail_leak = true,
                "security/subprocess-shell-true" => selection.subprocess_shell_true = true,
                "security/unsafe-yaml-load" => selection.unsafe_yaml_load = true,
                "security/weak-hash-without-flag" => selection.weak_hash_without_flag = true,
                "resilience/sqlalchemy-pool-pre-ping" => selection.sqlalchemy_pool_pre_ping = true,
                // New rules
                "resilience/bare-except-pass" => selection.bare_except_pass = true,
                "resilience/reraise-without-context" => selection.reraise_without_context = true,
                "resilience/exception-swallowed" => selection.exception_swallowed = true,
                "resilience/broad-except-no-context" => selection.broad_except_no_context = true,
                "security/sql-fstring-interpolation" => selection.sql_fstring_interpolation = true,
                "security/hardcoded-secret" => selection.hardcoded_secret = true,
                "security/pydantic-secretstr" => selection.pydantic_secretstr = true,
                "performance/sequential-awaits" => selection.sequential_awaits = true,
                "performance/regex-in-loop" => selection.regex_in_loop = true,
                "performance/n-plus-one-hint" => selection.n_plus_one_hint = true,
                "pydantic/deprecated-validator" => selection.deprecated_validator = true,
                "pydantic/mutable-default" => selection.mutable_model_default = true,
                "pydantic/extra-allow-on-request" => selection.extra_allow_on_request = true,
                "pydantic/should-be-model" => selection.should_be_model = true,
                "pydantic/sensitive-field-type" => selection.sensitive_field_type = true,
                "correctness/get-with-side-effect" => selection.get_with_side_effect = true,
                "correctness/serverless-filesystem-write" => selection.serverless_filesystem_write = true,
                "correctness/missing-http-timeout" => selection.missing_http_timeout = true,
                "architecture/god-module" => selection.god_module = true,
                "architecture/passthrough-function" => selection.passthrough_function = true,
                "architecture/avoid-sys-exit" => selection.avoid_sys_exit = true,
                "architecture/engine-pool-pre-ping" => selection.engine_pool_pre_ping = true,
                "architecture/missing-startup-validation" => selection.missing_startup_validation = true,
                "architecture/fat-route-handler" => selection.fat_route_handler = true,
                _ => {}
            }
        }
        selection
    }

    fn any_ast_rules(&self) -> bool {
        self.giant_function
            || self.deep_nesting
            || self.async_without_await
            || self.asyncio_run_in_async
            || self.sync_io_in_async
            || self.misused_async_constructs
            || self.mutable_default_arg
            || self.return_in_finally
            || self.threading_lock_in_async
            || self.unreachable_code
            // New AST rules
            || self.bare_except_pass
            || self.reraise_without_context
            || self.exception_swallowed
            || self.broad_except_no_context
            || self.sql_fstring_interpolation
            || self.hardcoded_secret
            || self.pydantic_secretstr
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
        self.print_in_production
            || self.star_import
            || self.direct_env_access
            || self.avoid_os_path
            || self.deprecated_typing_imports
            || self.naive_datetime
            || self.heavy_imports
            || self.assert_in_production
            || self.cors_wildcard
            || self.exception_detail_leak
            || self.subprocess_shell_true
            || self.unsafe_yaml_load
            || self.weak_hash_without_flag
            || self.sqlalchemy_pool_pre_ping
            // New line rules
            || self.deprecated_validator
            || self.extra_allow_on_request
            || self.missing_startup_validation
    }
}

fn analyze_module(
    module: &ModuleIndex,
    rules: &RuleSelection,
    config: &Config,
) -> Result<Vec<Issue>, String> {
    let mut issues = Vec::new();

    if rules.any_ast_rules() {
        issues.extend(rules::analyze_module_ast(module, rules, config)?);
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
            path: module.rel_path.clone(),
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

    // ── God module check (module-level, before line loop) ────────────
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
            path: module.rel_path.clone(),
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
        return Ok(issues);
    }

    let allow_print = rules.print_in_production && !module.has_path_part(&["scripts", "lib"]);
    let allow_star_import = rules.star_import && module.file_name.as_deref() != Some("__init__.py");
    let allow_direct_env =
        rules.direct_env_access && module.has_path_part(&["routers", "services", "interfaces"]);
    let allow_assert = rules.assert_in_production && !should_skip_assert(&module.rel_path);
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
                &module.rel_path,
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
                    &module.rel_path,
                    "CORSMiddleware with allow_origins=['*'] — any site can call your API",
                    "Specify explicit allowed origins: allow_origins=['https://yourdomain.com']",
                ));
            }
        }

        if allow_print
            && (line.trimmed_start.starts_with("print(")
                || line.trimmed_start.contains(" print(")
                || line.trimmed_start.contains("=print("))
        {
            issues.push(issue(
                "architecture/print-in-production",
                "warning",
                "Architecture",
                line.number,
                &module.rel_path,
                "print() in production code — use logger instead",
                "Replace with logger.info/debug/warning as appropriate.",
            ));
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
                path: module.rel_path.clone(),
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

        if rules.exception_detail_leak {
            let has_detail = line.compact.contains("detail=str(")
                || line.compact.contains("detail=f\"")
                || line.compact.contains("detail=f'");
            if line.compact.contains("HTTPException(") && has_detail {
                issues.push(issue(
                    "security/exception-detail-leak",
                    "warning",
                    "Security",
                    line.number,
                    &module.rel_path,
                    "Potential internal error leak in HTTPException detail",
                    "Use a generic error message. Log the real exception with logger.exception().",
                ));
            }
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
                    &module.rel_path,
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
                    &module.rel_path,
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
                        path: module.rel_path.clone(),
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
                        path: module.rel_path.clone(),
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
                    &module.rel_path,
                    "datetime.utcnow() is deprecated — use datetime.now(tz=UTC)",
                    "from datetime import UTC; datetime.now(tz=UTC)",
                ));
            } else if line.compact.contains("datetime.now()") {
                issues.push(issue(
                    "correctness/naive-datetime",
                    "warning",
                    "Correctness",
                    line.number,
                    &module.rel_path,
                    "datetime.now() without timezone — use datetime.now(tz=UTC)",
                    "from datetime import UTC; datetime.now(tz=UTC)",
                ));
            }
        }

        if rules.heavy_imports
            && (line.raw.starts_with("import ") || line.raw.starts_with("from "))
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
                        path: module.rel_path.clone(),
                        message: Box::leak(
                            format!(
                                "Heavy library {{'{}'}} imported at module level — degrades serverless cold-starts",
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
                        &module.rel_path,
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
                    &module.rel_path,
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
                    &module.rel_path,
                    "SQLAlchemy engine without pool_pre_ping=True",
                    "Add pool_pre_ping=True to create_engine() to ensure automatic recovery from dropped connections.",
                ));
            }
        }

        // ── New line-based rules ──────────────────────────────────────────

        if rules.deprecated_validator
            && line.trimmed.starts_with("@validator(")
            && !line.raw.contains("field_validator")
        {
            issues.push(issue(
                "pydantic/deprecated-validator",
                "error",
                "Pydantic",
                line.number,
                &module.rel_path,
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
                &module.rel_path,
                "Model in request path uses extra='allow' — accepts arbitrary user input",
                "Use extra='ignore' (drop unknown fields) or extra='forbid' (reject them).",
            ));
        }

        if rules.missing_startup_validation
            && module.file_name.as_deref() == Some("main.py")
            && line.number == 1
        {
            let has_validation = module.source.contains("validate_")
                && module.source.contains("startup")
                || module.source.contains("settings.validate")
                || module.source.contains("check_config")
                || module.source.contains("verify_env");
            if !has_validation {
                issues.push(issue(
                    "architecture/missing-startup-validation",
                    "warning",
                    "Architecture",
                    1,
                    &module.rel_path,
                    "Main app entry point missing explicit startup configuration validation",
                    "Add a 'fail-fast' validation step during app startup to verify critical settings.",
                ));
            }
        }
    }

    Ok(issues)
}

use pyo3::prelude::*;

#[pyfunction]
#[pyo3(signature = (
    import_bloat_threshold,
    giant_function_threshold,
    large_function_threshold,
    deep_nesting_threshold,
    god_module_threshold,
    fat_route_handler_threshold,
    should_be_model_mode,
    active_rules,
    modules
))]
fn analyze_modules(
    py: Python<'_>,
    import_bloat_threshold: usize,
    giant_function_threshold: usize,
    large_function_threshold: usize,
    deep_nesting_threshold: usize,
    god_module_threshold: usize,
    fat_route_handler_threshold: usize,
    should_be_model_mode: String,
    active_rules: Vec<String>,
    modules: Vec<(String, String)>,
) -> PyResult<Vec<(String, String, String, usize, String, String, String)>> {
    let config = Config {
        import_bloat_threshold,
        giant_function_threshold,
        large_function_threshold,
        deep_nesting_threshold,
        god_module_threshold,
        fat_route_handler_threshold,
        should_be_model_mode,
    };
    let rule_selection = RuleSelection::from_rules(&active_rules);

    use rayon::prelude::*;

    let all_issues: Result<Vec<Vec<Issue>>, String> = py.allow_threads(|| {
        let parsed_modules: Vec<ModuleRecord> = modules
            .into_iter()
            .map(|(rel_path, source)| ModuleRecord { rel_path, source })
            .collect();

        parsed_modules
            .par_iter()
            .map(|module| {
                let index = ModuleIndex::new(module);
                analyze_module(&index, &rule_selection, &config)
            })
            .collect()
    });

    let all_issues = all_issues.map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e))?;

    let mut out = Vec::new();
    for issues in all_issues {
        for issue in issues {
            out.push((
                issue.check.to_string(),
                issue.severity.to_string(),
                issue.category.to_string(),
                issue.line,
                issue.path,
                issue.message.to_string(),
                issue.help.to_string(),
            ));
        }
    }

    Ok(out)
}

#[pymodule]
fn _fastapi_doctor_native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(analyze_modules, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(path: &str, source: &str) -> ModuleRecord {
        ModuleRecord {
            rel_path: path.to_string(),
            source: source.to_string(),
        }
    }

    fn issues_for(rule_id: &str, path: &str, source: &str) -> Vec<Issue> {
        let config = Config {
            import_bloat_threshold: 3,
            giant_function_threshold: 400,
            large_function_threshold: 200,
            deep_nesting_threshold: 5, god_module_threshold: 500, fat_route_handler_threshold: 400, should_be_model_mode: "strict".to_string(),
        };
        issues_for_with_config(&[rule_id.to_string()], path, source, config)
    }

    fn issues_for_rules(rule_ids: &[&str], path: &str, source: &str) -> Vec<Issue> {
        let config = Config {
            import_bloat_threshold: 3,
            giant_function_threshold: 400,
            large_function_threshold: 200,
            deep_nesting_threshold: 5, god_module_threshold: 500, fat_route_handler_threshold: 400, should_be_model_mode: "strict".to_string(),
        };
        issues_for_with_config(
            &rule_ids
                .iter()
                .map(|rule_id| (*rule_id).to_string())
                .collect::<Vec<_>>(),
            path,
            source,
            config,
        )
    }

    fn issues_for_with_config(
        rule_ids: &[String],
        path: &str,
        source: &str,
        config: Config,
    ) -> Vec<Issue> {
        let module = module(path, source);
        let index = ModuleIndex::new(&module);
        let selection = RuleSelection::from_rules(rule_ids);
        analyze_module(&index, &selection, &config).unwrap()
    }




    #[test]
    fn assert_rule_skips_tests_and_flags_prod_code() {
        assert_eq!(
            issues_for(
                "security/assert-in-production",
                "tests/test_example.py",
                "assert value"
            ),
            Vec::<Issue>::new()
        );
        assert_eq!(
            issues_for(
                "security/assert-in-production",
                "app/main.py",
                "assert value"
            )
            .len(),
            1
        );
    }

    #[test]
    fn subprocess_rule_flags_shell_true() {
        let issues = issues_for(
            "security/subprocess-shell-true",
            "app/main.py",
            "import subprocess\nsubprocess.run(['echo', 'x'], shell=True)\n",
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].check, "security/subprocess-shell-true");
    }

    #[test]
    fn yaml_rule_respects_safe_loader() {
        assert_eq!(
            issues_for(
                "security/unsafe-yaml-load",
                "app/main.py",
                "import yaml\nyaml.load(data)\n",
            )
            .len(),
            1
        );
        assert_eq!(
            issues_for(
                "security/unsafe-yaml-load",
                "app/main.py",
                "import yaml\nyaml.load(data, Loader=yaml.SafeLoader)\n",
            )
            .len(),
            0
        );
    }

    #[test]
    fn weak_hash_rule_requires_security_flag() {
        assert_eq!(
            issues_for(
                "security/weak-hash-without-flag",
                "app/main.py",
                "import hashlib\nhashlib.md5(data).hexdigest()\n",
            )
            .len(),
            1
        );
        assert_eq!(
            issues_for(
                "security/weak-hash-without-flag",
                "app/main.py",
                "import hashlib\nhashlib.md5(data, usedforsecurity=False).hexdigest()\n",
            )
            .len(),
            0
        );
    }

    #[test]
    fn cors_wildcard_rule_honors_noqa() {
        assert_eq!(
            issues_for(
                "security/cors-wildcard",
                "app/main.py",
                "middleware = CORSMiddleware(app=None, allow_origins=['*'])\n",
            )
            .len(),
            1
        );
        assert_eq!(
            issues_for(
                "security/cors-wildcard",
                "app/main.py",
                "middleware = CORSMiddleware(app=None, allow_origins=['*'])  # noqa\n",
            )
            .len(),
            0
        );
    }

    #[test]
    fn exception_detail_rule_flags_detail_leaks() {
        let issues = issues_for(
            "security/exception-detail-leak",
            "app/main.py",
            "HTTPException(status_code=500, detail=str(exc))\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn os_path_rule_reports_called_attribute() {
        let issues = issues_for(
            "correctness/avoid-os-path",
            "app/main.py",
            "import os\nvalue = os.path.join('a', 'b')\n",
        );
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("join"));
    }

    #[test]
    fn typing_rule_finds_multiple_deprecated_imports() {
        let issues = issues_for(
            "correctness/deprecated-typing-imports",
            "app/main.py",
            "from typing import List, Optional, Dict\n",
        );
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("List"));
        assert!(issues[0].message.contains("Optional"));
        assert!(issues[0].message.contains("Dict"));
    }

    #[test]
    fn naive_datetime_rule_distinguishes_timezone_safe_calls() {
        assert_eq!(
            issues_for(
                "correctness/naive-datetime",
                "app/main.py",
                "from datetime import datetime\nvalue = datetime.utcnow()\n",
            )
            .len(),
            1
        );
        assert_eq!(
            issues_for(
                "correctness/naive-datetime",
                "app/main.py",
                "from datetime import datetime, UTC\nvalue = datetime.now(tz=UTC)\n",
            )
            .len(),
            0
        );
    }

    #[test]
    fn heavy_import_rule_matches_module_and_from_imports() {
        assert_eq!(
            issues_for(
                "performance/heavy-imports",
                "app/main.py",
                "import pandas\n"
            )
            .len(),
            1
        );
        assert_eq!(
            issues_for(
                "performance/heavy-imports",
                "app/main.py",
                "from torch import nn\n"
            )
            .len(),
            1
        );
    }

    #[test]
    fn direct_env_access_rule_only_applies_to_service_layers() {
        assert_eq!(
            issues_for(
                "config/direct-env-access",
                "pkg/services/settings.py",
                "import os\nvalue = os.environ['TOKEN']\n",
            )
            .len(),
            1
        );
        assert_eq!(
            issues_for(
                "config/direct-env-access",
                "pkg/config.py",
                "import os\nvalue = os.environ['TOKEN']\n",
            )
            .len(),
            0
        );
    }

    #[test]
    fn print_rule_skips_script_paths() {
        assert_eq!(
            issues_for(
                "architecture/print-in-production",
                "pkg/main.py",
                "print('x')\n"
            )
            .len(),
            1
        );
        assert_eq!(
            issues_for(
                "architecture/print-in-production",
                "scripts/run.py",
                "print('x')\n"
            )
            .len(),
            0
        );
    }

    #[test]
    fn star_import_rule_skips_init_modules() {
        assert_eq!(
            issues_for(
                "architecture/star-import",
                "pkg/mod.py",
                "from somewhere import *\n"
            )
            .len(),
            1
        );
        assert_eq!(
            issues_for(
                "architecture/star-import",
                "pkg/__init__.py",
                "from somewhere import *\n"
            )
            .len(),
            0
        );
    }

    #[test]
    fn import_bloat_rule_uses_threshold() {
        let source = "import a\nimport b\nimport c\nimport d\n";
        assert_eq!(
            issues_for("architecture/import-bloat", "pkg/mod.py", source).len(),
            1
        );
    }

    #[test]
    fn sqlalchemy_pool_pre_ping_rule_flags_missing_option() {
        assert_eq!(
            issues_for(
                "resilience/sqlalchemy-pool-pre-ping",
                "pkg/db.py",
                "engine = create_engine('sqlite://')\n",
            )
            .len(),
            1
        );
        assert_eq!(
            issues_for(
                "resilience/sqlalchemy-pool-pre-ping",
                "pkg/db.py",
                "engine = create_engine('sqlite://', pool_pre_ping=True)\n",
            )
            .len(),
            0
        );
    }



    #[test]
    fn async_without_await_rule_flags_transitive_helpers() {
        let issues = issues_for(
            "architecture/async-without-await",
            "pkg/services.py",
            "async def leaf():\n    return 1\n\nasync def middle():\n    return await leaf()\n\nasync def root():\n    return await middle()\n",
        );
        assert_eq!(issues.len(), 3);
        assert!(issues.iter().any(|issue| issue.message.contains("leaf")));
        assert!(issues.iter().any(|issue| issue.message.contains("middle")));
        assert!(issues.iter().any(|issue| issue.message.contains("root")));
    }

    #[test]
    fn sync_io_rule_flags_transitive_sync_helper_and_honors_suppression() {
        let issues = issues_for(
            "correctness/sync-io-in-async",
            "pkg/services.py",
            "import requests\n\ndef fetch_profile():\n    return requests.get('https://example.com')\n\nasync def load_profile():\n    return fetch_profile()  # doctor:ignore correctness/sync-io-in-async reason=\"legacy\"\n",
        );
        assert!(issues.is_empty());

        let issues = issues_for(
            "correctness/sync-io-in-async",
            "pkg/services.py",
            "import requests\n\ndef fetch_profile():\n    return requests.get('https://example.com')\n\nasync def load_profile():\n    return fetch_profile()\n",
        );
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("fetch_profile"));
        assert!(issues[0].help.contains("requests.get"));
    }

    #[test]
    fn misused_async_construct_rules_match_expected_cases() {
        let issues = issues_for_rules(
            &["correctness/misused-async-constructs"],
            "pkg/services.py",
            "from contextlib import contextmanager\n\ndef sync_helper():\n    return 1\n\ndef get_items():\n    return [1, 2, 3]\n\n@contextmanager\ndef sync_cm():\n    yield 'ok'\n\nasync def run_all():\n    await sync_helper()\n    async for item in get_items():\n        print(item)\n    async with sync_cm() as resource:\n        print(resource)\n",
        );
        let checks = issues.iter().map(|issue| issue.check).collect::<Vec<_>>();
        assert!(checks.contains(&"correctness/await-on-sync"));
        assert!(checks.contains(&"correctness/sync-iterable-in-async-for"));
        assert!(checks.contains(&"correctness/sync-cm-in-async-with"));
    }

    #[test]
    fn giant_function_rule_emits_large_function_warning() {
        let config = Config {
            import_bloat_threshold: 3,
            giant_function_threshold: 100,
            large_function_threshold: 3,
            deep_nesting_threshold: 5, god_module_threshold: 500, fat_route_handler_threshold: 400, should_be_model_mode: "strict".to_string(),
        };
        let issues = issues_for_with_config(
            &["architecture/giant-function".to_string()],
            "pkg/services.py",
            "def large():\n    x = 1\n    y = 2\n    z = 3\n    return x + y + z\n",
            config,
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].check, "architecture/large-function");
    }

    #[test]
    fn deep_nesting_rule_flags_control_flow_depth() {
        let issues = issues_for(
            "architecture/deep-nesting",
            "pkg/services.py",
            "def nested(flag):\n    if flag:\n        for item in [1]:\n            while item:\n                try:\n                    with open('x'):\n                        if item:\n                            return item\n                except Exception:\n                    return 0\n",
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].check, "architecture/deep-nesting");
    }

    #[test]
    fn asyncio_run_in_async_rule_skips_main_guard() {
        let issues = issues_for(
            "correctness/asyncio-run-in-async",
            "pkg/services.py",
            "import asyncio\n\nasync def main():\n    return 1\n\nasyncio.run(main())\n",
        );
        assert_eq!(issues.len(), 1);

        let issues = issues_for(
            "correctness/asyncio-run-in-async",
            "pkg/services.py",
            "import asyncio\n\nasync def main():\n    return 1\n\nif __name__ == '__main__':\n    asyncio.run(main())\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn threading_lock_in_async_rule_detects_direct_and_imported_lock() {
        let issues = issues_for(
            "correctness/threading-lock-in-async",
            "pkg/services.py",
            "import threading\n\nasync def main():\n    lock = threading.Lock()\n    return lock\n",
        );
        assert_eq!(issues.len(), 1);

        let issues = issues_for(
            "correctness/threading-lock-in-async",
            "pkg/services.py",
            "from threading import Lock\n\nasync def main():\n    lock = Lock()\n    return lock\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn mutable_default_arg_rule_flags_list_default() {
        let issues = issues_for(
            "correctness/mutable-default-arg",
            "pkg/services.py",
            "def build(items=[]):\n    return items\n",
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].check, "correctness/mutable-default-arg");
    }

    #[test]
    fn return_in_finally_rule_flags_nested_return() {
        let issues = issues_for(
            "correctness/return-in-finally",
            "pkg/services.py",
            "def build():\n    try:\n        return 1\n    finally:\n        if True:\n            return 2\n",
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].check, "correctness/return-in-finally");
    }

    #[test]
    fn unreachable_code_rule_flags_first_dead_statement() {
        let issues = issues_for(
            "correctness/unreachable-code",
            "pkg/services.py",
            "def build():\n    return 1\n    value = 2\n    other = 3\n",
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].check, "correctness/unreachable-code");
    }
}
