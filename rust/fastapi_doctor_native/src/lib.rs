use fastapi_doctor_core::{
    collect_suppressions, extract_route_scan, finalize_route, parse_suite, route_tuple,
    score_summary, Config, Issue, IssueTuple, RouteRecord, RouteTuple, SuppressionTuple,
};
use fastapi_doctor_project::{
    load_current_project_bundle, load_project_modules, resolve_project_context, LoadedProject,
    ProjectMetadata,
};
use fastapi_doctor_rules::{
    analyze_module, analyze_module_with_suite, analyze_project_modules, analyze_routes,
    select_rule_ids, RuleSelection,
};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use rayon::prelude::*;
use std::collections::HashMap;

#[pyfunction]
#[pyo3(signature = (
    import_bloat_threshold,
    giant_function_threshold,
    large_function_threshold,
    deep_nesting_threshold,
    god_module_threshold,
    fat_route_handler_threshold,
    should_be_model_mode,
    forbidden_write_params,
    create_post_prefixes,
    tag_required_prefixes,
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
    forbidden_write_params: Vec<String>,
    create_post_prefixes: Vec<String>,
    tag_required_prefixes: Vec<String>,
    active_rules: Vec<String>,
    modules: Vec<(String, String)>,
) -> PyResult<Vec<IssueTuple>> {
    let config = Config {
        import_bloat_threshold,
        giant_function_threshold,
        large_function_threshold,
        deep_nesting_threshold,
        god_module_threshold,
        fat_route_handler_threshold,
        should_be_model_mode,
        forbidden_write_params,
        create_post_prefixes,
        tag_required_prefixes,
    };
    let rule_selection = RuleSelection::from_rules(&active_rules);

    let all_issues: Result<Vec<Vec<Issue>>, String> = py.allow_threads(|| {
        modules
            .into_par_iter()
            .map(|(rel_path, source)| {
                let module = fastapi_doctor_core::ModuleRecord { rel_path, source };
                analyze_module(&module, &rule_selection, &config)
            })
            .collect()
    });

    let all_issues = all_issues.map_err(PyRuntimeError::new_err)?;
    Ok(all_issues
        .into_iter()
        .flatten()
        .map(|issue| issue_tuple(&issue))
        .collect())
}

#[pyfunction]
#[pyo3(signature = (
    repo_root,
    code_dir,
    excluded_dirs,
    import_bloat_threshold,
    giant_function_threshold,
    large_function_threshold,
    deep_nesting_threshold,
    god_module_threshold,
    fat_route_handler_threshold,
    should_be_model_mode,
    forbidden_write_params,
    create_post_prefixes,
    tag_required_prefixes,
    active_rules,
))]
fn analyze_project(
    py: Python<'_>,
    repo_root: String,
    code_dir: String,
    excluded_dirs: Vec<String>,
    import_bloat_threshold: usize,
    giant_function_threshold: usize,
    large_function_threshold: usize,
    deep_nesting_threshold: usize,
    god_module_threshold: usize,
    fat_route_handler_threshold: usize,
    should_be_model_mode: String,
    forbidden_write_params: Vec<String>,
    create_post_prefixes: Vec<String>,
    tag_required_prefixes: Vec<String>,
    active_rules: Vec<String>,
) -> PyResult<(Vec<IssueTuple>, Vec<RouteTuple>, Vec<SuppressionTuple>)> {
    let result = analyze_project_bundle(
        py,
        repo_root,
        code_dir,
        excluded_dirs,
        import_bloat_threshold,
        giant_function_threshold,
        large_function_threshold,
        deep_nesting_threshold,
        god_module_threshold,
        fat_route_handler_threshold,
        should_be_model_mode,
        forbidden_write_params,
        create_post_prefixes,
        tag_required_prefixes,
        active_rules,
        true,
    )?;
    Ok((result.issues, result.routes, result.suppressions))
}

#[pyfunction]
#[pyo3(signature = (
    repo_root,
    code_dir,
    excluded_dirs,
    import_bloat_threshold,
    giant_function_threshold,
    large_function_threshold,
    deep_nesting_threshold,
    god_module_threshold,
    fat_route_handler_threshold,
    should_be_model_mode,
    forbidden_write_params,
    create_post_prefixes,
    tag_required_prefixes,
    active_rules,
    include_routes=true,
))]
fn analyze_project_v2(
    py: Python<'_>,
    repo_root: String,
    code_dir: String,
    excluded_dirs: Vec<String>,
    import_bloat_threshold: usize,
    giant_function_threshold: usize,
    large_function_threshold: usize,
    deep_nesting_threshold: usize,
    god_module_threshold: usize,
    fat_route_handler_threshold: usize,
    should_be_model_mode: String,
    forbidden_write_params: Vec<String>,
    create_post_prefixes: Vec<String>,
    tag_required_prefixes: Vec<String>,
    active_rules: Vec<String>,
    include_routes: bool,
) -> PyResult<Py<PyDict>> {
    let result = analyze_project_bundle(
        py,
        repo_root,
        code_dir,
        excluded_dirs,
        import_bloat_threshold,
        giant_function_threshold,
        large_function_threshold,
        deep_nesting_threshold,
        god_module_threshold,
        fat_route_handler_threshold,
        should_be_model_mode,
        forbidden_write_params,
        create_post_prefixes,
        tag_required_prefixes,
        active_rules,
        include_routes,
    )?;

    let payload = PyDict::new(py);
    payload.set_item("issues", result.issues)?;
    payload.set_item("routes", result.routes)?;
    payload.set_item("suppressions", result.suppressions)?;
    payload.set_item("route_count", result.route_count)?;
    payload.set_item("openapi_path_count", py.None())?;
    let categories = PyDict::new(py);
    for (category, count) in result.categories {
        categories.set_item(category, count)?;
    }
    payload.set_item("categories", categories)?;
    payload.set_item("score", result.score)?;
    payload.set_item("label", result.label)?;
    payload.set_item("checks_not_evaluated", result.checks_not_evaluated)?;
    payload.set_item("engine_reason", result.engine_reason)?;
    Ok(payload.unbind())
}

#[pyfunction]
#[pyo3(signature = (
    active_rules,
    include_routes=true,
    static_only=true,
))]
fn analyze_current_project_v2(
    py: Python<'_>,
    active_rules: Vec<String>,
    include_routes: bool,
    static_only: bool,
) -> PyResult<Py<PyDict>> {
    let bundle = load_current_project_bundle(static_only).map_err(PyRuntimeError::new_err)?;
    let config = bundle.context.effective_config.to_core_config();
    let result = analyze_loaded_project_bundle(
        py,
        bundle.project,
        config,
        active_rules,
        include_routes,
        "using Rust-native auto project module v2",
    )?;
    let payload = project_bundle_payload(py, result)?;
    let project_context = project_context_payload(py, &bundle.context)?;
    payload
        .bind(py)
        .set_item("project_context", project_context)?;
    Ok(payload)
}

#[pyfunction]
#[pyo3(signature = (
    profile=None,
    only_rules=None,
    ignore_rules=None,
    skip_structure=false,
    skip_openapi=false,
    static_only=true,
))]
fn score_current_project_v2(
    py: Python<'_>,
    profile: Option<String>,
    only_rules: Option<Vec<String>>,
    ignore_rules: Option<Vec<String>>,
    skip_structure: bool,
    skip_openapi: bool,
    static_only: bool,
) -> PyResult<usize> {
    let (result, _) = analyze_selected_current_project_impl(
        py,
        profile,
        only_rules,
        ignore_rules,
        skip_structure,
        skip_openapi,
        static_only,
        false,
    )?;
    Ok(result.score)
}

struct ProjectBundleResult {
    issues: Vec<IssueTuple>,
    routes: Vec<RouteTuple>,
    suppressions: Vec<SuppressionTuple>,
    route_count: usize,
    categories: Vec<(String, usize)>,
    score: usize,
    label: String,
    checks_not_evaluated: Vec<String>,
    engine_reason: String,
}

struct ProjectAnalysisResult {
    raw_issues: Vec<Issue>,
    finalized_routes: Vec<RouteRecord>,
    suppressions: Vec<SuppressionTuple>,
    engine_reason: String,
}

impl ProjectAnalysisResult {
    fn into_bundle(self, include_route_payload: bool) -> ProjectBundleResult {
        let summary = score_summary(&self.raw_issues);
        let route_count = self.finalized_routes.len();
        let routes = if include_route_payload {
            self.finalized_routes.into_iter().map(route_tuple).collect()
        } else {
            Vec::new()
        };
        let issues = self.raw_issues.iter().map(issue_tuple).collect();
        ProjectBundleResult {
            route_count,
            categories: summary.categories.into_iter().collect(),
            score: summary.score,
            label: summary.label,
            issues,
            routes,
            suppressions: self.suppressions,
            checks_not_evaluated: Vec::new(),
            engine_reason: self.engine_reason,
        }
    }
}

fn empty_project_bundle_result(engine_reason: &str) -> ProjectBundleResult {
    ProjectBundleResult {
        issues: Vec::new(),
        routes: Vec::new(),
        suppressions: Vec::new(),
        route_count: 0,
        categories: Vec::new(),
        score: 100,
        label: "A".to_string(),
        checks_not_evaluated: Vec::new(),
        engine_reason: engine_reason.to_string(),
    }
}

fn source_may_define_routes(source: &str) -> bool {
    source.contains("APIRouter")
        || source.contains("FastAPI(")
        || source.contains("FastAPI (")
        || source.contains(".get(")
        || source.contains(".post(")
        || source.contains(".put(")
        || source.contains(".patch(")
        || source.contains(".delete(")
        || source.contains(".api_route(")
}

fn source_may_have_suppressions(source: &str) -> bool {
    source.contains("noqa") || source.contains("doctor:ignore")
}

fn analyze_project_bundle(
    py: Python<'_>,
    repo_root: String,
    code_dir: String,
    excluded_dirs: Vec<String>,
    import_bloat_threshold: usize,
    giant_function_threshold: usize,
    large_function_threshold: usize,
    deep_nesting_threshold: usize,
    god_module_threshold: usize,
    fat_route_handler_threshold: usize,
    should_be_model_mode: String,
    forbidden_write_params: Vec<String>,
    create_post_prefixes: Vec<String>,
    tag_required_prefixes: Vec<String>,
    active_rules: Vec<String>,
    include_routes: bool,
) -> PyResult<ProjectBundleResult> {
    let metadata = ProjectMetadata::new(repo_root.into(), code_dir.into(), excluded_dirs);
    let config = Config {
        import_bloat_threshold,
        giant_function_threshold,
        large_function_threshold,
        deep_nesting_threshold,
        god_module_threshold,
        fat_route_handler_threshold,
        should_be_model_mode,
        forbidden_write_params,
        create_post_prefixes,
        tag_required_prefixes,
    };
    let project = load_project_modules(metadata).map_err(PyRuntimeError::new_err)?;
    analyze_loaded_project_bundle(
        py,
        project,
        config,
        active_rules,
        include_routes,
        "using Rust-native project module v2",
    )
}

fn analyze_loaded_project_bundle(
    py: Python<'_>,
    project: LoadedProject,
    config: Config,
    active_rules: Vec<String>,
    include_routes: bool,
    engine_reason: &str,
) -> PyResult<ProjectBundleResult> {
    let analysis =
        analyze_loaded_project_bundle_core(py, project, config, active_rules, engine_reason)?;
    Ok(analysis.into_bundle(include_routes))
}

fn analyze_loaded_project_bundle_core(
    py: Python<'_>,
    project: LoadedProject,
    config: Config,
    active_rules: Vec<String>,
    engine_reason: &str,
) -> PyResult<ProjectAnalysisResult> {
    let rule_selection = RuleSelection::from_rules(&active_rules);
    let needs_routes = rule_selection.any_route_rules();

    let scans = py.allow_threads(|| {
        project
            .modules
            .par_iter()
            .map(|module| {
                let index = fastapi_doctor_core::ModuleIndex::new(module);
                let parsed_suite = parse_suite(module);
                let issues = analyze_module_with_suite(
                    &index,
                    parsed_suite.as_ref(),
                    &rule_selection,
                    &config,
                );
                let route_scan = if needs_routes && source_may_define_routes(&module.source) {
                    parsed_suite
                        .as_ref()
                        .map(|suite| extract_route_scan(&index, suite))
                        .unwrap_or_default()
                } else {
                    Default::default()
                };
                let suppressions = if source_may_have_suppressions(&module.source) {
                    collect_suppressions(&module.source, &module.rel_path)
                } else {
                    Vec::new()
                };
                Ok::<_, String>((issues, route_scan.drafts, suppressions, route_scan.includes))
            })
            .collect::<Result<Vec<_>, String>>()
    });

    let scans = scans.map_err(PyRuntimeError::new_err)?;
    let project_issues = analyze_project_modules(&project.modules, &rule_selection);

    let mut include_prefix_map: HashMap<String, (String, Vec<String>)> = HashMap::new();
    for (_, _, _, includes) in &scans {
        for (router_name, include_prefix, include_tags) in includes {
            match include_prefix_map.get(router_name) {
                Some((existing_prefix, _)) if existing_prefix.len() >= include_prefix.len() => {}
                _ => {
                    include_prefix_map.insert(
                        router_name.clone(),
                        (include_prefix.clone(), include_tags.clone()),
                    );
                }
            }
        }
    }

    let mut finalized_routes: Vec<RouteRecord> = Vec::new();
    let mut suppressions = Vec::new();
    let mut raw_issues = Vec::new();
    for (module_issues, module_routes, module_suppressions, _) in scans {
        raw_issues.extend(module_issues);
        for route in module_routes {
            finalized_routes.push(finalize_route(route, &include_prefix_map));
        }
        for suppression in module_suppressions {
            suppressions.push((
                suppression.rule,
                suppression.reason,
                suppression.path,
                suppression.line,
            ));
        }
    }
    raw_issues.extend(project_issues);

    if needs_routes {
        raw_issues.extend(analyze_routes(&finalized_routes, &rule_selection, &config));
    }

    Ok(ProjectAnalysisResult {
        raw_issues,
        finalized_routes,
        suppressions,
        engine_reason: engine_reason.to_string(),
    })
}

fn analyze_selected_current_project_impl(
    py: Python<'_>,
    profile: Option<String>,
    only_rules: Option<Vec<String>>,
    ignore_rules: Option<Vec<String>>,
    skip_structure: bool,
    skip_openapi: bool,
    static_only: bool,
    include_routes: bool,
) -> PyResult<(ProjectBundleResult, fastapi_doctor_project::ProjectContext)> {
    let bundle = load_current_project_bundle(static_only).map_err(PyRuntimeError::new_err)?;
    let active_rules = select_rule_ids(
        profile.as_deref(),
        only_rules.as_deref().unwrap_or(&[]),
        ignore_rules.as_deref().unwrap_or(&[]),
        &bundle.context.effective_config.scan.exclude_rules,
        skip_structure,
        skip_openapi,
    );
    if active_rules.is_empty() {
        return Ok((
            empty_project_bundle_result("no rules selected"),
            bundle.context,
        ));
    }

    let config = bundle.context.effective_config.to_core_config();
    let analysis = analyze_loaded_project_bundle_core(
        py,
        bundle.project,
        config,
        active_rules,
        "using Rust-native auto project module v2",
    )?;
    Ok((analysis.into_bundle(include_routes), bundle.context))
}

#[pyfunction]
#[pyo3(signature = (
    profile=None,
    only_rules=None,
    ignore_rules=None,
    skip_structure=false,
    skip_openapi=false,
    static_only=true,
    include_routes=true,
))]
fn analyze_selected_current_project_v2(
    py: Python<'_>,
    profile: Option<String>,
    only_rules: Option<Vec<String>>,
    ignore_rules: Option<Vec<String>>,
    skip_structure: bool,
    skip_openapi: bool,
    static_only: bool,
    include_routes: bool,
) -> PyResult<Py<PyDict>> {
    let (result, context) = analyze_selected_current_project_impl(
        py,
        profile,
        only_rules,
        ignore_rules,
        skip_structure,
        skip_openapi,
        static_only,
        include_routes,
    )?;
    let payload = project_bundle_payload(py, result)?;
    let project_context = project_context_payload(py, &context)?;
    payload
        .bind(py)
        .set_item("project_context", project_context)?;
    Ok(payload)
}

fn project_bundle_payload(py: Python<'_>, result: ProjectBundleResult) -> PyResult<Py<PyDict>> {
    let payload = PyDict::new(py);
    payload.set_item("issues", result.issues)?;
    payload.set_item("routes", result.routes)?;
    payload.set_item("suppressions", result.suppressions)?;
    payload.set_item("route_count", result.route_count)?;
    payload.set_item("openapi_path_count", py.None())?;
    let categories = PyDict::new(py);
    for (category, count) in result.categories {
        categories.set_item(category, count)?;
    }
    payload.set_item("categories", categories)?;
    payload.set_item("score", result.score)?;
    payload.set_item("label", result.label)?;
    payload.set_item("checks_not_evaluated", result.checks_not_evaluated)?;
    payload.set_item("engine_reason", result.engine_reason)?;
    Ok(payload.unbind())
}

fn project_context_payload(
    py: Python<'_>,
    context: &fastapi_doctor_project::ProjectContext,
) -> PyResult<Py<PyDict>> {
    let payload = PyDict::new(py);

    let layout = PyDict::new(py);
    layout.set_item(
        "repo_root",
        context.layout.repo_root.to_string_lossy().to_string(),
    )?;
    layout.set_item(
        "import_root",
        context.layout.import_root.to_string_lossy().to_string(),
    )?;
    layout.set_item(
        "code_dir",
        context.layout.code_dir.to_string_lossy().to_string(),
    )?;
    layout.set_item("app_module", context.layout.app_module.clone())?;
    layout.set_item("discovery_source", context.layout.discovery_source.clone())?;
    payload.set_item("layout", layout)?;

    let libraries = PyDict::new(py);
    libraries.set_item("fastapi", context.libraries.fastapi)?;
    libraries.set_item("pydantic", context.libraries.pydantic)?;
    libraries.set_item("sqlalchemy", context.libraries.sqlalchemy)?;
    libraries.set_item("sqlmodel", context.libraries.sqlmodel)?;
    libraries.set_item("django", context.libraries.django)?;
    libraries.set_item("flask", context.libraries.flask)?;
    libraries.set_item("httpx", context.libraries.httpx)?;
    libraries.set_item("requests", context.libraries.requests)?;
    libraries.set_item("alembic", context.libraries.alembic)?;
    libraries.set_item("pytest", context.libraries.pytest)?;
    libraries.set_item("ruff", context.libraries.ruff)?;
    libraries.set_item("mypy", context.libraries.mypy)?;
    payload.set_item("libraries", libraries)?;

    let effective_config = PyDict::new(py);
    let config_path = context
        .effective_config
        .config_path
        .as_ref()
        .map(|path: &std::path::PathBuf| path.to_string_lossy().to_string());
    effective_config.set_item("config_path", config_path)?;
    effective_config.set_item(
        "uses_legacy_config_name",
        context.effective_config.uses_legacy_config_name,
    )?;

    let architecture = PyDict::new(py);
    architecture.set_item("enabled", context.effective_config.architecture.enabled)?;
    architecture.set_item(
        "giant_function",
        context.effective_config.architecture.giant_function,
    )?;
    architecture.set_item(
        "large_function",
        context.effective_config.architecture.large_function,
    )?;
    architecture.set_item(
        "god_module",
        context.effective_config.architecture.god_module,
    )?;
    architecture.set_item(
        "deep_nesting",
        context.effective_config.architecture.deep_nesting,
    )?;
    architecture.set_item(
        "import_bloat",
        context.effective_config.architecture.import_bloat,
    )?;
    architecture.set_item(
        "fat_route_handler",
        context.effective_config.architecture.fat_route_handler,
    )?;
    effective_config.set_item("architecture", architecture)?;

    let pydantic = PyDict::new(py);
    pydantic.set_item(
        "should_be_model",
        context.effective_config.pydantic.should_be_model.clone(),
    )?;
    effective_config.set_item("pydantic", pydantic)?;

    let api = PyDict::new(py);
    api.set_item(
        "create_post_prefixes",
        context.effective_config.api.create_post_prefixes.clone(),
    )?;
    api.set_item(
        "tag_required_prefixes",
        context.effective_config.api.tag_required_prefixes.clone(),
    )?;
    effective_config.set_item("api", api)?;

    let security = PyDict::new(py);
    security.set_item(
        "forbidden_write_params",
        context
            .effective_config
            .security
            .forbidden_write_params
            .clone(),
    )?;
    effective_config.set_item("security", security)?;

    let scan = PyDict::new(py);
    scan.set_item(
        "exclude_dirs",
        context.effective_config.scan.exclude_dirs.clone(),
    )?;
    scan.set_item(
        "exclude_rules",
        context.effective_config.scan.exclude_rules.clone(),
    )?;
    effective_config.set_item("scan", scan)?;

    payload.set_item("effective_config", effective_config)?;
    Ok(payload.unbind())
}

fn issue_tuple(issue: &Issue) -> IssueTuple {
    (
        issue.check.to_string(),
        issue.severity.to_string(),
        issue.category.to_string(),
        issue.line,
        issue.path.clone(),
        issue.message.to_string(),
        issue.help.to_string(),
    )
}

#[pyfunction]
fn get_all_rule_ids() -> Vec<&'static str> {
    fastapi_doctor_rules::registry::StaticRule::all()
        .iter()
        .map(|r| r.rule_id())
        .collect()
}

#[pyfunction]
#[pyo3(signature = (static_only=false))]
fn get_project_context(py: Python<'_>, static_only: bool) -> PyResult<Py<PyDict>> {
    let context = resolve_project_context(static_only);
    project_context_payload(py, &context)
}

#[pymodule]
fn _fastapi_doctor_native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(analyze_modules, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_project, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_project_v2, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_current_project_v2, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_selected_current_project_v2, m)?)?;
    m.add_function(wrap_pyfunction!(score_current_project_v2, m)?)?;
    m.add_function(wrap_pyfunction!(get_all_rule_ids, m)?)?;
    m.add_function(wrap_pyfunction!(get_project_context, m)?)?;
    Ok(())
}
