#[cfg(test)]
mod rule_tests {
    use fastapi_doctor_core::{Config, Issue, ModuleRecord, RouteRecord};

    use crate::engine::{analyze_module, analyze_routes, RuleSelection};
    use crate::rule_selector::select_rule_ids;

    // ── Helpers ─────────────────────────────────────────────────────────

    fn module(path: &str, source: &str) -> ModuleRecord {
        ModuleRecord {
            rel_path: path.to_string(),
            source: source.to_string(),
        }
    }

    fn config() -> Config {
        Config {
            import_bloat_threshold: 3,
            giant_function_threshold: 400,
            large_function_threshold: 200,
            deep_nesting_threshold: 5,
            god_module_threshold: 1500,
            fat_route_handler_threshold: 100,
            should_be_model_mode: "strict".to_string(),
            ..Default::default()
        }
    }

    fn issues_for(rule_id: &str, path: &str, source: &str) -> Vec<Issue> {
        let m = module(path, source);
        let selection = RuleSelection::from_rules(&[rule_id.to_string()]);
        analyze_module(&m, &selection, &config()).unwrap()
    }

    fn issues_for_rules(rule_ids: &[&str], path: &str, source: &str) -> Vec<Issue> {
        let m = module(path, source);
        let rules: Vec<String> = rule_ids.iter().map(|s| s.to_string()).collect();
        let selection = RuleSelection::from_rules(&rules);
        analyze_module(&m, &selection, &config()).unwrap()
    }

    fn issues_for_with_config(rule_id: &str, path: &str, source: &str, cfg: Config) -> Vec<Issue> {
        let m = module(path, source);
        let selection = RuleSelection::from_rules(&[rule_id.to_string()]);
        analyze_module(&m, &selection, &cfg).unwrap()
    }

    fn route(
        path: &str,
        methods: &[&str],
        has_response_model: bool,
        status_code: Option<usize>,
        tags: &[&str],
        has_docstring: bool,
        param_names: &[&str],
        response_model_str: Option<&str>,
    ) -> RouteRecord {
        RouteRecord {
            path: path.to_string(),
            methods: methods.iter().map(|m| m.to_string()).collect(),
            dependency_names: vec![],
            param_names: param_names.iter().map(|p| p.to_string()).collect(),
            include_in_schema: true,
            has_response_model,
            response_model_str: response_model_str.map(|s| s.to_string()),
            status_code,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            endpoint_name: "handler".to_string(),
            has_docstring,
            source_file: "app/routes.py".to_string(),
            line: 1,
        }
    }

    fn route_issues(routes: &[RouteRecord], rule_ids: &[&str]) -> Vec<Issue> {
        let rules: Vec<String> = rule_ids.iter().map(|s| s.to_string()).collect();
        let selection = RuleSelection::from_rules(&rules);
        let cfg = config();
        analyze_routes(routes, &selection, &cfg)
    }

    // ── Security Rules ──────────────────────────────────────────────────

    #[test]
    fn unsafe_yaml_load_positive() {
        let issues = issues_for(
            "security/unsafe-yaml-load",
            "app/main.py",
            "import yaml\nyaml.load(data)\n",
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].check, "security/unsafe-yaml-load");
        assert_eq!(issues[0].severity, "error");
    }

    #[test]
    fn unsafe_yaml_load_negative_safe_loader() {
        let issues = issues_for(
            "security/unsafe-yaml-load",
            "app/main.py",
            "import yaml\nyaml.load(data, Loader=yaml.SafeLoader)\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn subprocess_shell_true_positive() {
        let issues = issues_for(
            "security/subprocess-shell-true",
            "app/main.py",
            "import subprocess\nsubprocess.run(['echo', 'x'], shell=True)\n",
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].check, "security/subprocess-shell-true");
    }

    #[test]
    fn subprocess_shell_true_negative() {
        let issues = issues_for(
            "security/subprocess-shell-true",
            "app/main.py",
            "import subprocess\nsubprocess.run(['echo', 'x'])\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn cors_wildcard_positive() {
        let issues = issues_for(
            "security/cors-wildcard",
            "app/main.py",
            "middleware = CORSMiddleware(app=None, allow_origins=['*'])\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn cors_wildcard_negative_specific_origin() {
        let issues = issues_for(
            "security/cors-wildcard",
            "app/main.py",
            "middleware = CORSMiddleware(app=None, allow_origins=['https://example.com'])\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn cors_wildcard_suppressed_with_noqa() {
        let issues = issues_for(
            "security/cors-wildcard",
            "app/main.py",
            "middleware = CORSMiddleware(app=None, allow_origins=['*'])  # noqa\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn weak_hash_without_flag_positive() {
        let issues = issues_for(
            "security/weak-hash-without-flag",
            "app/main.py",
            "import hashlib\nhashlib.md5(data).hexdigest()\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn weak_hash_without_flag_negative() {
        let issues = issues_for(
            "security/weak-hash-without-flag",
            "app/main.py",
            "import hashlib\nhashlib.md5(data, usedforsecurity=False).hexdigest()\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn assert_in_production_positive() {
        let issues = issues_for(
            "security/assert-in-production",
            "app/main.py",
            "assert value\n",
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, "error");
    }

    #[test]
    fn assert_in_production_negative_in_tests() {
        let issues = issues_for(
            "security/assert-in-production",
            "tests/test_example.py",
            "assert value\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn sql_fstring_interpolation_positive() {
        let issues = issues_for(
            "security/sql-fstring-interpolation",
            "app/main.py",
            "from sqlalchemy import text\nq = text(f\"SELECT * FROM users WHERE id = {uid}\")\n",
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].check, "security/sql-fstring-interpolation");
    }

    #[test]
    fn sql_fstring_interpolation_negative_plain_string() {
        let issues = issues_for(
            "security/sql-fstring-interpolation",
            "app/main.py",
            "from sqlalchemy import text\nq = text(\"SELECT * FROM users\")\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn hardcoded_secret_positive_known_prefix() {
        let issues = issues_for(
            "security/hardcoded-secret",
            "app/config.py",
            "API_KEY = \"sk_live_abc123def456ghi789\"\n",
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].check, "security/hardcoded-secret");
    }

    #[test]
    fn hardcoded_secret_negative_placeholder() {
        let issues = issues_for(
            "security/hardcoded-secret",
            "app/config.py",
            "API_KEY = \"your-api-key\"\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn hardcoded_secret_negative_test_file() {
        let issues = issues_for(
            "security/hardcoded-secret",
            "tests/test_auth.py",
            "API_KEY = \"sk_live_abc123def456ghi789\"\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn exception_detail_leak_positive() {
        let issues = issues_for(
            "security/exception-detail-leak",
            "app/main.py",
            "from fastapi import HTTPException\nHTTPException(status_code=500, detail=str(exc))\n",
        );
        assert_eq!(issues.len(), 1);
    }

    // ── Correctness Rules ───────────────────────────────────────────────

    #[test]
    fn mutable_default_arg_positive() {
        let issues = issues_for(
            "correctness/mutable-default-arg",
            "app/main.py",
            "def build(items=[]):\n    return items\n",
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].check, "correctness/mutable-default-arg");
    }

    #[test]
    fn mutable_default_arg_negative_none() {
        let issues = issues_for(
            "correctness/mutable-default-arg",
            "app/main.py",
            "def build(items=None):\n    return items or []\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn return_in_finally_positive() {
        let issues = issues_for(
            "correctness/return-in-finally",
            "app/main.py",
            "def build():\n    try:\n        return 1\n    finally:\n        return 2\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn return_in_finally_negative() {
        let issues = issues_for(
            "correctness/return-in-finally",
            "app/main.py",
            "def build():\n    try:\n        return 1\n    finally:\n        cleanup()\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn unreachable_code_positive() {
        let issues = issues_for(
            "correctness/unreachable-code",
            "app/main.py",
            "def build():\n    return 1\n    value = 2\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn unreachable_code_negative() {
        let issues = issues_for(
            "correctness/unreachable-code",
            "app/main.py",
            "def build():\n    value = 2\n    return value\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn naive_datetime_utcnow_positive() {
        let issues = issues_for(
            "correctness/naive-datetime",
            "app/main.py",
            "from datetime import datetime\nvalue = datetime.utcnow()\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn naive_datetime_now_with_tz_negative() {
        let issues = issues_for(
            "correctness/naive-datetime",
            "app/main.py",
            "from datetime import datetime, UTC\nvalue = datetime.now(tz=UTC)\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn deprecated_typing_imports_positive() {
        let issues = issues_for(
            "correctness/deprecated-typing-imports",
            "app/main.py",
            "from typing import List, Optional, Dict\n",
        );
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("List"));
        assert!(issues[0].message.contains("Dict"));
    }

    #[test]
    fn deprecated_typing_imports_negative_builtins() {
        let issues = issues_for(
            "correctness/deprecated-typing-imports",
            "app/main.py",
            "from typing import Annotated, TypeVar\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn avoid_os_path_positive() {
        let issues = issues_for(
            "correctness/avoid-os-path",
            "app/main.py",
            "import os\nvalue = os.path.join('a', 'b')\n",
        );
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("join"));
    }

    #[test]
    fn asyncio_run_in_async_positive() {
        let issues = issues_for(
            "correctness/asyncio-run-in-async",
            "app/services.py",
            "import asyncio\n\nasync def main():\n    return 1\n\nasyncio.run(main())\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn asyncio_run_in_async_negative_main_guard() {
        let issues = issues_for(
            "correctness/asyncio-run-in-async",
            "app/services.py",
            "import asyncio\n\nasync def main():\n    return 1\n\nif __name__ == '__main__':\n    asyncio.run(main())\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn threading_lock_in_async_positive() {
        let issues = issues_for(
            "correctness/threading-lock-in-async",
            "app/main.py",
            "import threading\n\nasync def main():\n    lock = threading.Lock()\n    return lock\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn threading_lock_in_async_positive_from_import() {
        let issues = issues_for(
            "correctness/threading-lock-in-async",
            "app/main.py",
            "from threading import Lock\n\nasync def main():\n    lock = Lock()\n    return lock\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn missing_http_timeout_positive() {
        let issues = issues_for(
            "correctness/missing-http-timeout",
            "app/main.py",
            "import requests\nresponse = requests.get('https://example.com')\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn missing_http_timeout_negative() {
        let issues = issues_for(
            "correctness/missing-http-timeout",
            "app/main.py",
            "import requests\nresponse = requests.get('https://example.com', timeout=30)\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn sync_io_in_async_positive_direct() {
        let issues = issues_for(
            "correctness/sync-io-in-async",
            "app/main.py",
            "import time\n\nasync def handler():\n    time.sleep(1)\n",
        );
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("time.sleep"));
    }

    #[test]
    fn sync_io_in_async_positive_transitive() {
        let issues = issues_for(
            "correctness/sync-io-in-async",
            "app/main.py",
            "import requests\n\ndef fetch_profile():\n    return requests.get('https://example.com')\n\nasync def load_profile():\n    return fetch_profile()\n",
        );
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("fetch_profile"));
    }

    #[test]
    fn sync_io_in_async_negative_with_suppression() {
        let issues = issues_for(
            "correctness/sync-io-in-async",
            "app/main.py",
            "import requests\n\ndef fetch():\n    return requests.get('https://example.com')\n\nasync def load():\n    return fetch()  # doctor:ignore correctness/sync-io-in-async reason=\"legacy\"\n",
        );
        assert!(issues.is_empty());
    }

    // ── Architecture Rules ──────────────────────────────────────────────

    #[test]
    fn giant_function_positive() {
        let mut source = "def huge():\n".to_string();
        for i in 0..450 {
            source.push_str(&format!("    x{} = {}\n", i, i));
        }
        let issues = issues_for("architecture/giant-function", "app/main.py", &source);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].check, "architecture/giant-function");
        assert_eq!(issues[0].severity, "error");
    }

    #[test]
    fn large_function_positive() {
        let cfg = Config {
            large_function_threshold: 10,
            giant_function_threshold: 100,
            ..config()
        };
        let mut source = "def medium():\n".to_string();
        for i in 0..15 {
            source.push_str(&format!("    x{} = {}\n", i, i));
        }
        let issues =
            issues_for_with_config("architecture/giant-function", "app/main.py", &source, cfg);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].check, "architecture/large-function");
        assert_eq!(issues[0].severity, "warning");
    }

    #[test]
    fn deep_nesting_positive() {
        let issues = issues_for(
            "architecture/deep-nesting",
            "app/main.py",
            "def nested(flag):\n    if flag:\n        for item in [1]:\n            while item:\n                try:\n                    with open('x'):\n                        if item:\n                            return item\n                except Exception:\n                    return 0\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn deep_nesting_negative_shallow() {
        let issues = issues_for(
            "architecture/deep-nesting",
            "app/main.py",
            "def shallow():\n    if True:\n        return 1\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn print_in_production_positive() {
        let issues = issues_for(
            "architecture/print-in-production",
            "app/main.py",
            "print('hello')\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn print_in_production_negative_scripts() {
        let issues = issues_for(
            "architecture/print-in-production",
            "scripts/run.py",
            "print('hello')\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn star_import_positive() {
        let issues = issues_for(
            "architecture/star-import",
            "app/mod.py",
            "from somewhere import *\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn star_import_negative_init() {
        let issues = issues_for(
            "architecture/star-import",
            "app/__init__.py",
            "from somewhere import *\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn import_bloat_positive() {
        let source = "import a\nimport b\nimport c\nimport d\n";
        let issues = issues_for("architecture/import-bloat", "app/mod.py", source);
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn import_bloat_negative_below_threshold() {
        let source = "import a\nimport b\n";
        let issues = issues_for("architecture/import-bloat", "app/mod.py", source);
        assert!(issues.is_empty());
    }

    #[test]
    fn god_module_positive() {
        let mut source = String::new();
        for i in 0..1600 {
            source.push_str(&format!("x{} = {}\n", i, i));
        }
        let issues = issues_for("architecture/god-module", "app/mod.py", &source);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].check, "architecture/god-module");
    }

    #[test]
    fn async_without_await_positive() {
        let issues = issues_for(
            "architecture/async-without-await",
            "app/main.py",
            "async def leaf():\n    return 1\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn async_without_await_positive_transitive() {
        let issues = issues_for(
            "architecture/async-without-await",
            "app/main.py",
            "async def leaf():\n    return 1\n\nasync def middle():\n    return await leaf()\n\nasync def root():\n    return await middle()\n",
        );
        assert_eq!(issues.len(), 3);
    }

    #[test]
    fn avoid_sys_exit_positive() {
        let issues = issues_for(
            "architecture/avoid-sys-exit",
            "app/main.py",
            "import sys\nsys.exit(1)\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn avoid_sys_exit_negative_cli() {
        let issues = issues_for(
            "architecture/avoid-sys-exit",
            "app/cli.py",
            "import sys\nsys.exit(1)\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn avoid_sys_exit_negative_main() {
        let issues = issues_for(
            "architecture/avoid-sys-exit",
            "app/__main__.py",
            "import sys\nsys.exit(1)\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn engine_pool_pre_ping_positive() {
        let issues = issues_for(
            "architecture/engine-pool-pre-ping",
            "app/db.py",
            "from sqlalchemy import create_engine\nengine = create_engine('sqlite://')\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn engine_pool_pre_ping_negative() {
        let issues = issues_for(
            "architecture/engine-pool-pre-ping",
            "app/db.py",
            "from sqlalchemy import create_engine\nengine = create_engine('sqlite://', pool_pre_ping=True)\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn missing_startup_validation_positive() {
        let issues = issues_for(
            "architecture/missing-startup-validation",
            "app/main.py",
            "from fastapi import FastAPI\n\napp = FastAPI()\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn missing_startup_validation_negative_settings() {
        let issues = issues_for(
            "architecture/missing-startup-validation",
            "app/main.py",
            "from fastapi import FastAPI\nfrom app.core.config import settings\n\napp = FastAPI(title=settings.PROJECT_NAME)\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn missing_startup_validation_negative_router_module() {
        let issues = issues_for(
            "architecture/missing-startup-validation",
            "app/api/main.py",
            "from fastapi import APIRouter\n\napi_router = APIRouter()\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn passthrough_function_positive() {
        let issues = issues_for(
            "architecture/passthrough-function",
            "app/main.py",
            "def wrapper(a, b, c):\n    return inner(a, b, c)\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn passthrough_function_negative_with_docstring() {
        let issues = issues_for(
            "architecture/passthrough-function",
            "app/main.py",
            "def wrapper(a, b, c):\n    \"\"\"Documented reason.\"\"\"\n    return inner(a, b, c)\n",
        );
        assert!(issues.is_empty());
    }

    // ── Performance Rules ───────────────────────────────────────────────

    #[test]
    fn heavy_imports_positive_pandas() {
        let issues = issues_for(
            "performance/heavy-imports",
            "app/main.py",
            "import pandas\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn heavy_imports_positive_from_torch() {
        let issues = issues_for(
            "performance/heavy-imports",
            "app/main.py",
            "from torch import nn\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn heavy_imports_negative_stdlib() {
        let issues = issues_for("performance/heavy-imports", "app/main.py", "import json\n");
        assert!(issues.is_empty());
    }

    #[test]
    fn sequential_awaits_positive() {
        let issues = issues_for(
            "performance/sequential-awaits",
            "app/main.py",
            "async def handler():\n    a = await fetch_a()\n    b = await fetch_b()\n    return a, b\n",
        );
        assert_eq!(issues.len(), 1);
    }

    // ── Pydantic Rules ──────────────────────────────────────────────────

    #[test]
    fn deprecated_validator_positive() {
        let issues = issues_for(
            "pydantic/deprecated-validator",
            "app/models.py",
            "@validator('name')\ndef validate_name(cls, v):\n    return v\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn extra_allow_on_request_positive() {
        let issues = issues_for(
            "pydantic/extra-allow-on-request",
            "app/routers/models.py",
            "class Config:\n    extra=\"allow\"\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn extra_allow_on_request_negative_non_router() {
        let issues = issues_for(
            "pydantic/extra-allow-on-request",
            "app/models.py",
            "class Config:\n    extra=\"allow\"\n",
        );
        assert!(issues.is_empty());
    }

    // ── Resilience Rules ────────────────────────────────────────────────

    #[test]
    fn sqlalchemy_pool_pre_ping_positive() {
        let issues = issues_for(
            "resilience/sqlalchemy-pool-pre-ping",
            "app/db.py",
            "engine = create_engine('sqlite://')\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn sqlalchemy_pool_pre_ping_negative() {
        let issues = issues_for(
            "resilience/sqlalchemy-pool-pre-ping",
            "app/db.py",
            "engine = create_engine('sqlite://', pool_pre_ping=True)\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn bare_except_pass_positive() {
        let issues = issues_for(
            "resilience/bare-except-pass",
            "app/main.py",
            "try:\n    do_work()\nexcept:\n    pass\n",
        );
        assert_eq!(issues.len(), 1);
    }

    // ── Config Rules ────────────────────────────────────────────────────

    #[test]
    fn direct_env_access_positive_in_services() {
        let issues = issues_for(
            "config/direct-env-access",
            "app/services/auth.py",
            "import os\nvalue = os.environ['TOKEN']\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn direct_env_access_negative_in_config() {
        let issues = issues_for(
            "config/direct-env-access",
            "app/config.py",
            "import os\nvalue = os.environ['TOKEN']\n",
        );
        assert!(issues.is_empty());
    }

    // ── Route Rules ─────────────────────────────────────────────────────

    #[test]
    fn duplicate_route_positive() {
        let routes = vec![
            route(
                "/api/users",
                &["GET"],
                true,
                None,
                &["users"],
                true,
                &[],
                None,
            ),
            route(
                "/api/users",
                &["GET"],
                true,
                None,
                &["users"],
                true,
                &[],
                None,
            ),
        ];
        let issues = route_issues(&routes, &["correctness/duplicate-route"]);
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn duplicate_route_negative_different_methods() {
        let routes = vec![
            route(
                "/api/users",
                &["GET"],
                true,
                None,
                &["users"],
                true,
                &[],
                None,
            ),
            route(
                "/api/users",
                &["POST"],
                true,
                Some(201),
                &["users"],
                true,
                &[],
                None,
            ),
        ];
        let issues = route_issues(&routes, &["correctness/duplicate-route"]);
        assert!(issues.is_empty());
    }

    #[test]
    fn missing_response_model_positive() {
        let routes = vec![route(
            "/api/users",
            &["GET"],
            false,
            None,
            &["users"],
            true,
            &[],
            None,
        )];
        let issues = route_issues(&routes, &["correctness/missing-response-model"]);
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn missing_response_model_negative_has_model() {
        let routes = vec![route(
            "/api/users",
            &["GET"],
            true,
            None,
            &["users"],
            true,
            &[],
            Some("list[user]"),
        )];
        let issues = route_issues(&routes, &["correctness/missing-response-model"]);
        assert!(issues.is_empty());
    }

    #[test]
    fn serverless_filesystem_write_negative_safe_tmp_constant_and_helper() {
        let issues = issues_for(
            "correctness/serverless-filesystem-write",
            "app/cache.py",
            "from pathlib import Path\n\
from app.fs import atomic_write_text\n\
ROOT = Path('/tmp') / 'app'\n\
TARGET = ROOT / 'state.json'\n\
atomic_write_text(TARGET, '{}')\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn serverless_filesystem_write_negative_serverless_helper_root() {
        let issues = issues_for(
            "correctness/serverless-filesystem-write",
            "app/cache.py",
            "from app.fs import atomic_write_bytes, serverless_temp_root\n\
path = serverless_temp_root('app', 'state.bin')\n\
atomic_write_bytes(path, b'data')\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn serverless_filesystem_write_positive_non_tmp_prompt_path() {
        let issues = issues_for(
            "correctness/serverless-filesystem-write",
            "app/prompts.py",
            "from pathlib import Path\n\
from app.fs import atomic_write_text\n\
PROMPTS = Path(__file__).resolve().parent / 'prompts'\n\
atomic_write_text(PROMPTS / 'base.md', 'hello')\n",
        );
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn missing_tags_positive() {
        let mut cfg = config();
        cfg.tag_required_prefixes = vec!["/api/".to_string()];
        let routes = vec![route(
            "/api/users",
            &["GET"],
            true,
            None,
            &[],
            true,
            &[],
            None,
        )];
        let rules: Vec<String> = vec!["api-surface/missing-tags".to_string()];
        let selection = RuleSelection::from_rules(&rules);
        let issues = analyze_routes(&routes, &selection, &cfg);
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn missing_tags_negative_has_tags() {
        let mut cfg = config();
        cfg.tag_required_prefixes = vec!["/api/".to_string()];
        let routes = vec![route(
            "/api/users",
            &["GET"],
            true,
            None,
            &["users"],
            true,
            &[],
            None,
        )];
        let rules: Vec<String> = vec!["api-surface/missing-tags".to_string()];
        let selection = RuleSelection::from_rules(&rules);
        let issues = analyze_routes(&routes, &selection, &cfg);
        assert!(issues.is_empty());
    }

    #[test]
    fn missing_docstring_positive() {
        let routes = vec![route(
            "/api/users",
            &["GET"],
            true,
            None,
            &["users"],
            false,
            &[],
            None,
        )];
        let issues = route_issues(&routes, &["api-surface/missing-docstring"]);
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn missing_pagination_positive() {
        let routes = vec![route(
            "/api/users",
            &["GET"],
            true,
            None,
            &["users"],
            true,
            &[],
            Some("list[user]"),
        )];
        let issues = route_issues(&routes, &["api-surface/missing-pagination"]);
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn missing_pagination_negative_has_params() {
        let routes = vec![route(
            "/api/users",
            &["GET"],
            true,
            None,
            &["users"],
            true,
            &["limit", "offset"],
            Some("list[user]"),
        )];
        let issues = route_issues(&routes, &["api-surface/missing-pagination"]);
        assert!(issues.is_empty());
    }

    #[test]
    fn post_status_code_positive() {
        let mut cfg = config();
        cfg.create_post_prefixes = vec!["/api/".to_string()];
        let routes = vec![route(
            "/api/users",
            &["POST"],
            true,
            None,
            &["users"],
            true,
            &[],
            None,
        )];
        let rules: Vec<String> = vec!["correctness/post-status-code".to_string()];
        let selection = RuleSelection::from_rules(&rules);
        let issues = analyze_routes(&routes, &selection, &cfg);
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn post_status_code_negative_201() {
        let mut cfg = config();
        cfg.create_post_prefixes = vec!["/api/".to_string()];
        let routes = vec![route(
            "/api/users",
            &["POST"],
            true,
            Some(201),
            &["users"],
            true,
            &[],
            None,
        )];
        let rules: Vec<String> = vec!["correctness/post-status-code".to_string()];
        let selection = RuleSelection::from_rules(&rules);
        let issues = analyze_routes(&routes, &selection, &cfg);
        assert!(issues.is_empty());
    }

    #[test]
    fn forbidden_write_param_positive() {
        let mut cfg = config();
        cfg.forbidden_write_params = vec!["user_id".to_string()];
        let routes = vec![route(
            "/api/users",
            &["POST"],
            true,
            Some(201),
            &["users"],
            true,
            &["user_id", "name"],
            None,
        )];
        let rules: Vec<String> = vec!["security/forbidden-write-param".to_string()];
        let selection = RuleSelection::from_rules(&rules);
        let issues = analyze_routes(&routes, &selection, &cfg);
        assert_eq!(issues.len(), 1);
    }

    // ── Rule Selection / Profiles ───────────────────────────────────────

    #[test]
    fn select_rule_ids_strict_returns_all() {
        let ids = select_rule_ids(None, &[], &[], &[], false, false);
        assert!(ids.len() >= 50);
        assert!(ids.contains(&"security/unsafe-yaml-load".to_string()));
        assert!(ids.contains(&"architecture/giant-function".to_string()));
        assert!(ids.contains(&"architecture/large-function".to_string()));
        assert!(ids.contains(&"api-surface/missing-tags".to_string()));
    }

    #[test]
    fn select_rule_ids_security_profile() {
        let ids = select_rule_ids(Some("security"), &[], &[], &[], false, false);
        assert!(ids.contains(&"security/unsafe-yaml-load".to_string()));
        assert!(ids.contains(&"security/cors-wildcard".to_string()));
        assert!(!ids.contains(&"architecture/giant-function".to_string()));
        assert!(!ids.contains(&"performance/heavy-imports".to_string()));
    }

    #[test]
    fn select_rule_ids_balanced_profile() {
        let ids = select_rule_ids(Some("balanced"), &[], &[], &[], false, false);
        assert!(ids.contains(&"security/unsafe-yaml-load".to_string()));
        assert!(ids.contains(&"correctness/mutable-default-arg".to_string()));
        assert!(ids.contains(&"resilience/bare-except-pass".to_string()));
        assert!(ids.contains(&"api-surface/missing-tags".to_string()));
        assert!(ids.contains(&"api-surface/missing-docstring".to_string()));
        // Architecture rules not in balanced unless explicitly listed
        assert!(!ids.contains(&"architecture/giant-function".to_string()));
    }

    #[test]
    fn select_rule_ids_only_rules_overrides() {
        let ids = select_rule_ids(
            Some("security"),
            &["architecture/giant-function".to_string()],
            &[],
            &[],
            false,
            false,
        );
        assert_eq!(ids, vec!["architecture/giant-function"]);
    }

    #[test]
    fn select_rule_ids_ignore_rules_excludes() {
        let ids = select_rule_ids(
            None,
            &[],
            &["security/unsafe-yaml-load".to_string()],
            &[],
            false,
            false,
        );
        assert!(!ids.contains(&"security/unsafe-yaml-load".to_string()));
        assert!(ids.contains(&"security/cors-wildcard".to_string()));
    }

    #[test]
    fn select_rule_ids_skip_structure() {
        let ids = select_rule_ids(None, &[], &[], &[], true, false);
        assert!(!ids.contains(&"architecture/giant-function".to_string()));
        assert!(!ids.contains(&"security/unsafe-yaml-load".to_string()));
        assert!(!ids.contains(&"correctness/mutable-default-arg".to_string()));
        // api-surface should still be present
        assert!(ids.contains(&"api-surface/missing-tags".to_string()));
    }

    #[test]
    fn select_rule_ids_skip_openapi() {
        let ids = select_rule_ids(None, &[], &[], &[], false, true);
        assert!(!ids.contains(&"api-surface/missing-tags".to_string()));
        assert!(ids.contains(&"security/unsafe-yaml-load".to_string()));
    }

    #[test]
    fn select_rule_ids_wildcard_ignore() {
        let ids = select_rule_ids(None, &[], &["security/*".to_string()], &[], false, false);
        assert!(!ids.iter().any(|id| id.starts_with("security/")));
    }

    // ── Suppression Integration ─────────────────────────────────────────

    #[test]
    fn suppression_doctor_ignore_exact_rule() {
        // AST-based rules (hardcoded-secret) use is_rule_suppressed() which handles doctor:ignore
        let issues = issues_for(
            "security/hardcoded-secret",
            "app/main.py",
            "api_key = \"sk_live_1234567890abcdef\"  # doctor:ignore security/hardcoded-secret reason=\"legacy\"\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn suppression_noqa_blanket() {
        // AST-based rules (mutable-default-arg) use is_rule_suppressed() which handles blanket noqa
        let issues = issues_for(
            "correctness/mutable-default-arg",
            "app/main.py",
            "def foo(items=[]):  # noqa\n    pass\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn suppression_noqa_exact_rule() {
        // AST-based rules (hardcoded-secret) use is_rule_suppressed() which handles exact noqa
        let issues = issues_for(
            "security/hardcoded-secret",
            "app/main.py",
            "api_key = \"sk_live_1234567890abcdef\"  # noqa: security/hardcoded-secret\n",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn suppression_noqa_category_alias() {
        let issues = issues_for(
            "security/cors-wildcard",
            "app/main.py",
            "CORSMiddleware(app=None, allow_origins=['*'])  # noqa: security\n",
        );
        assert!(issues.is_empty());
    }

    // ── Multiple Rules Interaction ──────────────────────────────────────

    #[test]
    fn multiple_rules_fire_simultaneously() {
        let issues = issues_for_rules(
            &[
                "security/unsafe-yaml-load",
                "security/subprocess-shell-true",
            ],
            "app/main.py",
            "import yaml\nimport subprocess\nyaml.load(data)\nsubprocess.run(['echo'], shell=True)\n",
        );
        assert_eq!(issues.len(), 2);
        let checks: Vec<&str> = issues.iter().map(|i| i.check).collect();
        assert!(checks.contains(&"security/unsafe-yaml-load"));
        assert!(checks.contains(&"security/subprocess-shell-true"));
    }
}
