use std::env;
use std::fs;
use std::path::Path;

const NATIVE_VERSION: &str = match option_env!("FASTAPI_DOCTOR_NATIVE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

#[derive(Clone)]
struct ModuleRecord {
    rel_path: String,
    source: String,
}

#[derive(Clone, Default)]
struct Config {
    import_bloat_threshold: usize,
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
        let file_name = path.file_name().map(|name| name.to_string_lossy().into_owned());

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

        Self {
            rel_path: module.rel_path.clone(),
            source: module.source.clone(),
            lines,
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

fn decode_hex(input: &str) -> Result<String, String> {
    if input.len() % 2 != 0 {
        return Err("hex input must have even length".to_string());
    }

    let mut bytes = Vec::with_capacity(input.len() / 2);
    let chars: Vec<char> = input.chars().collect();
    let mut idx = 0;
    while idx < chars.len() {
        let pair = format!("{}{}", chars[idx], chars[idx + 1]);
        let byte = u8::from_str_radix(&pair, 16).map_err(|err| err.to_string())?;
        bytes.push(byte);
        idx += 2;
    }

    String::from_utf8(bytes).map_err(|err| err.to_string())
}

fn encode_hex(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for byte in input.as_bytes() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn normalized_no_space(line: &str) -> String {
    line.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn should_skip_assert(path: &str) -> bool {
    path.contains("tests/") || path.contains("alembic/")
}

fn check_assert_in_production(module: &ModuleIndex) -> Vec<Issue> {
    if should_skip_assert(&module.rel_path) {
        return Vec::new();
    }
    if !module.source.contains("assert ") && !module.source.contains("assert(") {
        return Vec::new();
    }

    module
        .lines
        .iter()
        .filter(|line| line.trimmed_start.starts_with("assert ") || line.trimmed_start.starts_with("assert("))
        .map(|line| {
            issue(
                "security/assert-in-production",
                "error",
                "Security",
                line.number,
                &module.rel_path,
                "assert statement outside tests — use explicit exception raises",
                "Asserts are ignored when Python runs with -O. Raise ValueError or custom exceptions instead. Do not wrap in 'if condition:' without raising, as that silently skips the check.",
            )
        })
        .collect()
}

fn check_cors_wildcard(module: &ModuleIndex) -> Vec<Issue> {
    if !module.source.contains("CORSMiddleware") {
        return Vec::new();
    }

    module
        .lines
        .iter()
        .filter(|line| {
            let has_cors =
                line.compact.contains("CORSMiddleware(") || line.compact.contains(".add_middleware(CORSMiddleware");
            let wildcard_origins =
                line.compact.contains("allow_origins=[\"*\"]") || line.compact.contains("allow_origins=['*']");
            has_cors && wildcard_origins && !line.raw.contains("# noqa")
        })
        .map(|line| {
            issue(
                "security/cors-wildcard",
                "warning",
                "Security",
                line.number,
                &module.rel_path,
                "CORSMiddleware with allow_origins=['*'] — any site can call your API",
                "Specify explicit allowed origins: allow_origins=['https://yourdomain.com']",
            )
        })
        .collect()
}

fn check_print_in_production(module: &ModuleIndex) -> Vec<Issue> {
    if !module.source.contains("print(") || module.has_path_part(&["scripts", "lib"]) {
        return Vec::new();
    }

    module
        .lines
        .iter()
        .filter(|line| {
            line.trimmed_start.starts_with("print(")
                || line.trimmed_start.contains(" print(")
                || line.trimmed_start.contains("=print(")
        })
        .map(|line| {
            issue(
                "architecture/print-in-production",
                "warning",
                "Architecture",
                line.number,
                &module.rel_path,
                "print() in production code — use logger instead",
                "Replace with logger.info/debug/warning as appropriate.",
            )
        })
        .collect()
}

fn check_star_import(module: &ModuleIndex) -> Vec<Issue> {
    if !module.source.contains("import *")
        || module.file_name.as_deref() == Some("__init__.py")
    {
        return Vec::new();
    }

    module
        .lines
        .iter()
        .filter_map(|line| {
            if !(line.trimmed.starts_with("from ") && line.trimmed.contains(" import *") && !line.raw.contains("# noqa")) {
                return None;
            }
            let module_name = line
                .trimmed
                .strip_prefix("from ")
                .and_then(|rest| rest.split(" import *").next())
                .unwrap_or("module");
            Some(Issue {
                check: "architecture/star-import",
                severity: "warning",
                category: "Architecture",
                line: line.number,
                path: module.rel_path.clone(),
                message: Box::leak(
                    format!("from {} import * — pollutes namespace and breaks static analysis", module_name)
                        .into_boxed_str(),
                ),
                help: "Import specific names: from module import Name1, Name2",
            })
        })
        .collect()
}

fn check_import_bloat(module: &ModuleIndex, threshold: usize) -> Vec<Issue> {
    if threshold == 0
        || module.file_name.as_deref() == Some("__init__.py")
        || module.file_name.as_deref() == Some("main.py")
        || module.has_noqa_architecture
    {
        return Vec::new();
    }

    if module.import_count > threshold {
        vec![Issue {
            check: "architecture/import-bloat",
            severity: "warning",
            category: "Architecture",
            line: 0,
            path: module.rel_path.clone(),
            message: Box::leak(
                format!("File has {} imports (>{}) — consider decomposing", module.import_count, threshold)
                    .into_boxed_str(),
            ),
            help: "Use TYPE_CHECKING guards for type-only imports, lazy-import heavy libraries, or split the module.",
        }]
    } else {
        Vec::new()
    }
}

fn check_exception_detail_leak(module: &ModuleIndex) -> Vec<Issue> {
    if !module.source.contains("HTTPException") {
        return Vec::new();
    }

    module
        .lines
        .iter()
        .filter(|line| {
            let has_detail = line.compact.contains("detail=str(")
                || line.compact.contains("detail=f\"")
                || line.compact.contains("detail=f'");
            line.compact.contains("HTTPException(") && has_detail
        })
        .map(|line| {
            issue(
                "security/exception-detail-leak",
                "warning",
                "Security",
                line.number,
                &module.rel_path,
                "Potential internal error leak in HTTPException detail",
                "Use a generic error message. Log the real exception with logger.exception().",
            )
        })
        .collect()
}

fn check_shell_true(module: &ModuleIndex) -> Vec<Issue> {
    if !module.source.contains("subprocess") {
        return Vec::new();
    }

    module
        .lines
        .iter()
        .filter(|line| {
            let has_target = line.compact.contains("subprocess.Popen(")
                || line.compact.contains("subprocess.run(")
                || line.compact.contains("subprocess.call(")
                || line.compact.contains("subprocess.check_call(")
                || line.compact.contains("subprocess.check_output(");
            has_target && line.compact.contains("shell=True")
        })
        .map(|line| {
            issue(
                "security/subprocess-shell-true",
                "error",
                "Security",
                line.number,
                &module.rel_path,
                "subprocess executed with shell=True — potential shell injection",
                "Pass arguments as a list and remove shell=True to avoid injection risks.",
            )
        })
        .collect()
}

fn check_unsafe_yaml_load(module: &ModuleIndex) -> Vec<Issue> {
    if !module.source.contains("yaml.load(") {
        return Vec::new();
    }

    module
        .lines
        .iter()
        .filter(|line| {
            let safe_loader = line.compact.contains("Loader=yaml.SafeLoader")
                || line.compact.contains("Loader=yaml.BaseLoader")
                || line.compact.contains("Loader=yaml.CSafeLoader");
            line.raw.contains("yaml.load(") && !line.raw.contains("nosec") && !safe_loader
        })
        .map(|line| {
            issue(
                "security/unsafe-yaml-load",
                "error",
                "Security",
                line.number,
                &module.rel_path,
                "yaml.load() without SafeLoader/BaseLoader allows arbitrary code execution",
                "Use yaml.safe_load() or pass Loader=yaml.SafeLoader.",
            )
        })
        .collect()
}

fn check_avoid_os_path(module: &ModuleIndex) -> Vec<Issue> {
    if !module.source.contains("os.path") {
        return Vec::new();
    }

    module
        .lines
        .iter()
        .filter_map(|line| {
            let rest = line.compact.split("os.path.").nth(1)?;
            let attr: String = rest
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                .collect();
            if attr.is_empty() {
                return None;
            }
            Some(Issue {
                check: "correctness/avoid-os-path",
                severity: "warning",
                category: "Correctness",
                line: line.number,
                path: module.rel_path.clone(),
                message: Box::leak(
                    format!("os.path.{} usage detected — prefer pathlib.Path", attr).into_boxed_str(),
                ),
                help: "pathlib offers a safer, more robust object-oriented API for paths.",
            })
        })
        .collect()
}

fn check_deprecated_typing_imports(module: &ModuleIndex) -> Vec<Issue> {
    if !module.source.contains("from typing import") {
        return Vec::new();
    }

    let deprecated = [
        "List",
        "Dict",
        "Tuple",
        "Set",
        "FrozenSet",
        "Type",
        "Optional",
        "Union",
    ];

    module
        .lines
        .iter()
        .filter_map(|line| {
            if !line.trimmed.starts_with("from typing import") || line.raw.contains("# noqa") {
                return None;
            }
            let imported = line.trimmed.strip_prefix("from typing import")?.trim();
            let found: Vec<&str> = imported
                .split(',')
                .map(|part| part.trim())
                .filter(|name| deprecated.contains(name))
                .collect();
            if found.is_empty() {
                return None;
            }
            Some(Issue {
                check: "correctness/deprecated-typing-imports",
                severity: "warning",
                category: "Correctness",
                line: line.number,
                path: module.rel_path.clone(),
                message: Box::leak(
                    format!("Deprecated typing imports: {} — use builtins", found.join(", ")).into_boxed_str(),
                ),
                help: "Use list, dict, tuple, set, X | None directly. Add 'from __future__ import annotations' for 3.7+ compat.",
            })
        })
        .collect()
}

fn check_naive_datetime(module: &ModuleIndex) -> Vec<Issue> {
    if !module.source.contains("datetime") {
        return Vec::new();
    }

    module
        .lines
        .iter()
        .filter_map(|line| {
            if line.compact.contains("datetime.utcnow()") {
                return Some(issue(
                    "correctness/naive-datetime",
                    "warning",
                    "Correctness",
                    line.number,
                    &module.rel_path,
                    "datetime.utcnow() is deprecated — use datetime.now(tz=UTC)",
                    "from datetime import UTC; datetime.now(tz=UTC)",
                ));
            }
            if line.compact.contains("datetime.now()") {
                return Some(issue(
                    "correctness/naive-datetime",
                    "warning",
                    "Correctness",
                    line.number,
                    &module.rel_path,
                    "datetime.now() without timezone — use datetime.now(tz=UTC)",
                    "from datetime import UTC; datetime.now(tz=UTC)",
                ));
            }
            None
        })
        .collect()
}

fn check_heavy_imports(module: &ModuleIndex) -> Vec<Issue> {
    let heavy_libs = ["agno", "openai", "pandas", "numpy", "torch", "transformers", "playwright", "langchain"];

    module
        .lines
        .iter()
        .filter_map(|line| {
            if !(line.trimmed_start.starts_with("import ") || line.trimmed_start.starts_with("from ")) {
                return None;
            }
            for lib in heavy_libs {
                let import_prefix = format!("import {}", lib);
                let from_prefix = format!("from {}", lib);
                if line.trimmed_start.starts_with(&import_prefix) || line.trimmed_start.starts_with(&from_prefix) {
                    return Some(Issue {
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
                }
            }
            None
        })
        .collect()
}

fn check_direct_env_access(module: &ModuleIndex) -> Vec<Issue> {
    if !module.has_path_part(&["routers", "services", "interfaces"]) {
        return Vec::new();
    }

    module
        .lines
        .iter()
        .filter(|line| {
            if !line.trimmed.contains("os.environ") {
                return false;
            }
            if line.trimmed.contains("# noqa: direct-env")
                || line.trimmed.contains("os.environ.setdefault")
                || line.trimmed.contains("]= ")
                || line.trimmed.contains("] =")
            {
                return false;
            }

            let get_pos = line.trimmed.find("os.environ.get(");
            let bracket_pos = line.trimmed.find("os.environ[");
            let reads_bracket = bracket_pos.is_some();
            let reads_get = if let Some(pos) = get_pos {
                !line.trimmed[pos..].contains(',')
            } else {
                false
            };
            reads_bracket || reads_get
        })
        .map(|line| {
            issue(
                "config/direct-env-access",
                "warning",
                "Config",
                line.number,
                &module.rel_path,
                "Direct os.environ access in service/router code — use settings object",
                "Read env vars in one config/settings module, then inject the typed setting where needed.",
            )
        })
        .collect()
}

fn check_unsafe_hash_usage(module: &ModuleIndex) -> Vec<Issue> {
    if !module.source.contains("sha1") && !module.source.contains("md5") {
        return Vec::new();
    }

    module
        .lines
        .iter()
        .filter(|line| {
            let uses_hash = line.compact.contains("sha1(") || line.compact.contains("md5(");
            let uses_hexdigest = line.compact.contains(".hexdigest()");
            let has_flag = line.compact.contains("usedforsecurity=False");
            !line.raw.contains("nosec") && uses_hash && uses_hexdigest && !has_flag
        })
        .map(|line| {
            issue(
                "security/weak-hash-without-flag",
                "error",
                "Security",
                line.number,
                &module.rel_path,
                "SHA1/MD5 used without usedforsecurity=False",
                "Add usedforsecurity=False to signal this is not for security purposes.",
            )
        })
        .collect()
}

fn check_sqlalchemy_pool_pre_ping(module: &ModuleIndex) -> Vec<Issue> {
    if !module.source.contains("create_engine") {
        return Vec::new();
    }

    module
        .lines
        .iter()
        .filter(|line| {
            let is_engine_call =
                line.compact.contains("create_engine(") || line.compact.contains(".create_engine(");
            is_engine_call && !line.compact.contains("pool_pre_ping=True")
        })
        .map(|line| {
            issue(
                "resilience/sqlalchemy-pool-pre-ping",
                "warning",
                "Resilience",
                line.number,
                &module.rel_path,
                "SQLAlchemy engine without pool_pre_ping=True",
                "Add pool_pre_ping=True to create_engine() to ensure automatic recovery from dropped connections.",
            )
        })
        .collect()
}

fn run_rule(rule_id: &str, module: &ModuleIndex, config: &Config) -> Vec<Issue> {
    match rule_id {
        "architecture/import-bloat" => check_import_bloat(module, config.import_bloat_threshold),
        "architecture/print-in-production" => check_print_in_production(module),
        "architecture/star-import" => check_star_import(module),
        "config/direct-env-access" => check_direct_env_access(module),
        "correctness/avoid-os-path" => check_avoid_os_path(module),
        "correctness/deprecated-typing-imports" => check_deprecated_typing_imports(module),
        "correctness/naive-datetime" => check_naive_datetime(module),
        "performance/heavy-imports" => check_heavy_imports(module),
        "security/assert-in-production" => check_assert_in_production(module),
        "security/cors-wildcard" => check_cors_wildcard(module),
        "security/exception-detail-leak" => check_exception_detail_leak(module),
        "security/subprocess-shell-true" => check_shell_true(module),
        "security/unsafe-yaml-load" => check_unsafe_yaml_load(module),
        "security/weak-hash-without-flag" => check_unsafe_hash_usage(module),
        "resilience/sqlalchemy-pool-pre-ping" => check_sqlalchemy_pool_pre_ping(module),
        _ => Vec::new(),
    }
}

fn parse_request(path: &Path) -> Result<(Config, Vec<String>, Vec<ModuleRecord>), String> {
    let content = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let mut config = Config {
        import_bloat_threshold: 30,
    };
    let mut rules = Vec::new();
    let mut modules = Vec::new();

    for line in content.lines() {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        match parts.first().copied() {
            Some("VERSION") => {}
            Some("CONFIG") => {
                if parts.len() != 3 {
                    return Err("invalid CONFIG line".to_string());
                }
                if parts[1] == "IMPORT_BLOAT_THRESHOLD" {
                    config.import_bloat_threshold = parts[2].parse::<usize>().map_err(|err| err.to_string())?;
                }
            }
            Some("RULE") => {
                if parts.len() != 2 {
                    return Err("invalid RULE line".to_string());
                }
                rules.push(parts[1].to_string());
            }
            Some("MODULE") => {
                if parts.len() != 3 {
                    return Err("invalid MODULE line".to_string());
                }
                modules.push(ModuleRecord {
                    rel_path: decode_hex(parts[1])?,
                    source: decode_hex(parts[2])?,
                });
            }
            _ => return Err(format!("unknown request line: {line}")),
        }
    }

    Ok((config, rules, modules))
}

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let first_arg = args
        .next()
        .ok_or_else(|| "usage: fastapi-doctor-native <request-file>".to_string())?;
    if first_arg == "--version" {
        println!("{NATIVE_VERSION}");
        return Ok(());
    }

    let request_path = first_arg;
    let (config, rules, modules) = parse_request(Path::new(&request_path))?;

    let mut issues = Vec::new();
    for module in &modules {
        let index = ModuleIndex::new(module);
        for rule in &rules {
            issues.extend(run_rule(rule, &index, &config));
        }
    }

    for issue in issues {
        println!(
            "ISSUE\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            issue.check,
            issue.severity,
            issue.category,
            issue.line,
            encode_hex(&issue.path),
            encode_hex(issue.message),
            encode_hex(issue.help),
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn module(path: &str, source: &str) -> ModuleRecord {
        ModuleRecord {
            rel_path: path.to_string(),
            source: source.to_string(),
        }
    }

    fn issues_for(rule_id: &str, path: &str, source: &str) -> Vec<Issue> {
        let config = Config {
            import_bloat_threshold: 3,
        };
        let module = module(path, source);
        let index = ModuleIndex::new(&module);
        run_rule(rule_id, &index, &config)
    }

    #[test]
    fn version_flag_uses_native_version_env_or_package_version() {
        assert!(!NATIVE_VERSION.is_empty());
    }

    #[test]
    fn hex_round_trip_preserves_unicode() {
        let encoded = encode_hex("hello-æøå");
        assert_eq!(decode_hex(&encoded).unwrap(), "hello-æøå");
    }

    #[test]
    fn assert_rule_skips_tests_and_flags_prod_code() {
        assert_eq!(
            issues_for("security/assert-in-production", "tests/test_example.py", "assert value"),
            Vec::<Issue>::new()
        );
        assert_eq!(
            issues_for("security/assert-in-production", "app/main.py", "assert value").len(),
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
            issues_for("performance/heavy-imports", "app/main.py", "import pandas\n").len(),
            1
        );
        assert_eq!(
            issues_for("performance/heavy-imports", "app/main.py", "from torch import nn\n").len(),
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
            issues_for("architecture/print-in-production", "pkg/main.py", "print('x')\n").len(),
            1
        );
        assert_eq!(
            issues_for("architecture/print-in-production", "scripts/run.py", "print('x')\n").len(),
            0
        );
    }

    #[test]
    fn star_import_rule_skips_init_modules() {
        assert_eq!(
            issues_for("architecture/star-import", "pkg/mod.py", "from somewhere import *\n").len(),
            1
        );
        assert_eq!(
            issues_for("architecture/star-import", "pkg/__init__.py", "from somewhere import *\n").len(),
            0
        );
    }

    #[test]
    fn import_bloat_rule_uses_threshold() {
        let source = "import a\nimport b\nimport c\nimport d\n";
        assert_eq!(issues_for("architecture/import-bloat", "pkg/mod.py", source).len(), 1);
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
    fn parse_request_reads_config_rules_and_modules() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let request_path = env::temp_dir().join(format!("fastapi-doctor-native-{unique}.txt"));
        let module_path = encode_hex("pkg/main.py");
        let source = encode_hex("print('x')\n");
        fs::write(
            &request_path,
            format!(
                "VERSION\t1\nCONFIG\tIMPORT_BLOAT_THRESHOLD\t9\nRULE\tarchitecture/print-in-production\nMODULE\t{module_path}\t{source}\n"
            ),
        )
        .unwrap();

        let (config, rules, modules) = parse_request(&request_path).unwrap();
        fs::remove_file(request_path).unwrap();

        assert_eq!(config.import_bloat_threshold, 9);
        assert_eq!(rules, vec!["architecture/print-in-production"]);
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].rel_path, "pkg/main.py");
        assert_eq!(modules[0].source, "print('x')\n");
    }
}
