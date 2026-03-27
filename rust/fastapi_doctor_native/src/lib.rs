use std::collections::HashMap;
use std::path::PathBuf;

use fastapi_doctor_core::{
    Config, Issue, IssueTuple, RouteTuple, SuppressionTuple, collect_suppressions,
    extract_route_scan, finalize_route, parse_suite, score_summary,
};
use fastapi_doctor_project::{ProjectMetadata, load_project_modules, resolve_project_context};
use fastapi_doctor_rules::{
    RuleSelection, analyze_module, analyze_module_with_suite, analyze_project_modules,
};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use rayon::prelude::*;

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
) -> PyResult<Vec<IssueTuple>> {
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
    active_rules: Vec<String>,
    include_routes: bool,
) -> PyResult<ProjectBundleResult> {
    let metadata = ProjectMetadata::new(
        PathBuf::from(repo_root),
        PathBuf::from(code_dir),
        excluded_dirs,
    );
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

    let project = load_project_modules(metadata).map_err(PyRuntimeError::new_err)?;
    let scans = py.allow_threads(|| {
        project
            .modules
            .par_iter()
            .map(|module| {
                let index = fastapi_doctor_core::ModuleIndex::new(module);
                let parsed_suite = parse_suite(module);
                let issues =
                    analyze_module_with_suite(&index, parsed_suite.as_ref(), &rule_selection, &config);
                let route_scan = if include_routes {
                    parsed_suite
                        .as_ref()
                        .map(|suite| extract_route_scan(&index, suite))
                        .unwrap_or_default()
                } else {
                    Default::default()
                };
                let suppressions = collect_suppressions(&module.source, &module.rel_path);
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

    let mut issues = Vec::new();
    let mut routes = Vec::new();
    let mut suppressions = Vec::new();
    let mut raw_issues = Vec::new();
    for (module_issues, module_routes, module_suppressions, _) in scans {
        for issue in module_issues {
            issues.push(issue_tuple(&issue));
            raw_issues.push(issue);
        }
        for route in module_routes {
            routes.push(finalize_route(route, &include_prefix_map));
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
    for issue in project_issues {
        issues.push(issue_tuple(&issue));
        raw_issues.push(issue);
    }

    let summary = score_summary(&raw_issues);
    Ok(ProjectBundleResult {
        route_count: routes.len(),
        categories: summary.categories.into_iter().collect(),
        score: summary.score,
        label: summary.label,
        issues,
        routes,
        suppressions,
        checks_not_evaluated: Vec::new(),
        engine_reason: "using PyO3 native project module v2".to_string(),
    })
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
    let payload = PyDict::new(py);

    let layout = PyDict::new(py);
    layout.set_item("repo_root", context.layout.repo_root.to_string_lossy().to_string())?;
    layout.set_item("import_root", context.layout.import_root.to_string_lossy().to_string())?;
    layout.set_item("code_dir", context.layout.code_dir.to_string_lossy().to_string())?;
    layout.set_item("app_module", context.layout.app_module)?;
    layout.set_item("discovery_source", context.layout.discovery_source)?;
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

    Ok(payload.unbind())
}

#[pymodule]
fn _fastapi_doctor_native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(analyze_modules, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_project, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_project_v2, m)?)?;
    m.add_function(wrap_pyfunction!(get_all_rule_ids, m)?)?;
    m.add_function(wrap_pyfunction!(get_project_context, m)?)?;
    Ok(())
}
