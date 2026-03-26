pub(crate) mod architecture;
pub(crate) mod correctness;
pub(crate) mod performance;
pub(crate) mod pydantic;
pub(crate) mod resilience;
pub(crate) mod security;

use rustpython_parser::ast::{self};
use rustpython_parser::Parse;

use crate::{Config, Issue, ModuleIndex, RuleSelection};
use crate::ast_helpers::FunctionIndex;

pub(crate) fn analyze_module_ast<'a>(
    module: &ModuleIndex<'a>,
    rules: &RuleSelection,
    config: &Config,
) -> Result<Vec<Issue>, String> {
    if !rules.any_ast_rules() {
        return Ok(Vec::new());
    }

    let suite = match ast::Suite::parse(&module.source, &module.rel_path) {
        Ok(suite) => suite,
        Err(_) => return Ok(Vec::new()), // skip unparseable modules (e.g. complex f-strings)
    };

    let mut issues = Vec::new();

    if rules.giant_function
        && (config.giant_function_threshold > 0 || config.large_function_threshold > 0)
    {
        issues.extend(architecture::collect_giant_function_issues(module, &suite, config));
    }

    if rules.deep_nesting && config.deep_nesting_threshold > 0 {
        issues.extend(architecture::collect_deep_nesting_issues(
            module,
            &suite,
            config.deep_nesting_threshold,
        ));
    }

    if rules.asyncio_run_in_async {
        issues.extend(correctness::collect_asyncio_run_in_async_issues(module, &suite));
    }
    if rules.threading_lock_in_async {
        issues.extend(correctness::collect_threading_lock_in_async_issues(module, &suite));
    }
    if rules.mutable_default_arg {
        issues.extend(correctness::collect_mutable_default_arg_issues(module, &suite));
    }
    if rules.return_in_finally {
        issues.extend(correctness::collect_return_in_finally_issues(module, &suite));
    }
    if rules.unreachable_code {
        issues.extend(correctness::collect_unreachable_code_issues(module, &suite));
    }

    // ── New AST rules (simple, no function index needed) ──────────────
    if rules.bare_except_pass || rules.reraise_without_context
        || rules.exception_swallowed || rules.broad_except_no_context
    {
        issues.extend(resilience::collect_resilience_issues(module, &suite, rules));
    }
    if rules.sql_fstring_interpolation {
        issues.extend(security::collect_sql_fstring_issues(module, &suite));
    }
    if rules.hardcoded_secret {
        issues.extend(security::collect_hardcoded_secret_issues(module, &suite));
    }
    if rules.pydantic_secretstr || rules.sensitive_field_type
        || rules.mutable_model_default || rules.should_be_model
    {
        issues.extend(pydantic::collect_pydantic_issues(module, &suite, rules, config));
    }
    if rules.avoid_sys_exit {
        issues.extend(architecture::collect_avoid_sys_exit_issues(module, &suite));
    }
    if rules.engine_pool_pre_ping {
        issues.extend(architecture::collect_engine_pool_pre_ping_issues(module, &suite));
    }
    if rules.serverless_filesystem_write {
        issues.extend(correctness::collect_serverless_filesystem_write_issues(module, &suite));
    }
    if rules.missing_http_timeout {
        issues.extend(correctness::collect_missing_http_timeout_issues(module, &suite));
    }
    if rules.regex_in_loop {
        issues.extend(performance::collect_regex_in_loop_issues(module, &suite));
    }
    if rules.n_plus_one_hint {
        issues.extend(performance::collect_n_plus_one_hint_issues(module, &suite));
    }
    if rules.get_with_side_effect {
        issues.extend(correctness::collect_get_with_side_effect_issues(module, &suite));
    }
    if rules.fat_route_handler {
        issues.extend(architecture::collect_fat_route_handler_issues(module, &suite, config));
    }
    if rules.passthrough_function {
        issues.extend(architecture::collect_passthrough_function_issues(module, &suite));
    }
    if rules.sequential_awaits {
        issues.extend(performance::collect_sequential_awaits_issues(module, &suite));
    }

    // ── Function-index-dependent rules ──────────────────────────────
    if !(rules.async_without_await || rules.sync_io_in_async || rules.misused_async_constructs) {
        return Ok(issues);
    }

    let function_index = FunctionIndex::from_suite(module, &suite);

    if rules.async_without_await {
        issues.extend(architecture::collect_async_without_await_issues(module, &function_index));
    }
    if rules.sync_io_in_async {
        issues.extend(correctness::collect_sync_io_in_async_issues(module, &function_index));
    }
    if rules.misused_async_constructs {
        issues.extend(correctness::collect_misused_async_construct_issues(
            module,
            &function_index,
        ));
    }

    Ok(issues)
}
