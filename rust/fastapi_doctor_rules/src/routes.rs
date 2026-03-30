use std::collections::{HashMap, HashSet};

use fastapi_doctor_core::{Config, Issue, RouteRecord};

use crate::engine::RuleSelection;

pub(crate) fn analyze_routes(
    routes: &[RouteRecord],
    rules: &RuleSelection,
    config: &Config,
) -> Vec<Issue> {
    let mut issues = Vec::new();

    if rules.missing_auth_dep && auth_rule_configured(config) {
        issues.extend(collect_missing_auth_dep_issues(routes, config));
    }
    if rules.forbidden_write_param && !config.forbidden_write_params.is_empty() {
        issues.extend(collect_forbidden_write_param_issues(routes, config));
    }
    if rules.duplicate_route {
        issues.extend(collect_duplicate_route_issues(routes));
    }
    if rules.missing_response_model {
        issues.extend(collect_missing_response_model_issues(routes));
    }
    if rules.weak_response_model {
        issues.extend(collect_weak_response_model_issues(routes));
    }
    if rules.post_status_code && !config.create_post_prefixes.is_empty() {
        issues.extend(collect_post_status_code_issues(routes, config));
    }
    if rules.missing_tags {
        issues.extend(collect_missing_tag_issues(routes, config));
    }
    if rules.missing_docstring {
        issues.extend(collect_missing_docstring_issues(routes));
    }
    if rules.missing_pagination {
        issues.extend(collect_missing_pagination_issues(routes));
    }

    issues
}

pub(crate) fn route_checks_not_evaluated(rules: &RuleSelection, config: &Config) -> Vec<String> {
    let mut checks = Vec::new();
    if rules.missing_auth_dep && !auth_rule_configured(config) {
        checks.push("security/missing-auth-dep".to_string());
    }
    checks
}

fn auth_rule_configured(config: &Config) -> bool {
    !config.auth_required_prefixes.is_empty() && !config.auth_dependency_names.is_empty()
}

fn is_response_model_exempt_path(path: &str) -> bool {
    [
        "/stream",
        "/export",
        "-export",
        "/download",
        "/webhook",
        "/oauth",
        "/callback",
        "/{",
    ]
    .iter()
    .any(|pattern| path.contains(pattern))
}

fn collect_missing_auth_dep_issues(routes: &[RouteRecord], config: &Config) -> Vec<Issue> {
    let required_dependencies: HashSet<&str> = config
        .auth_dependency_names
        .iter()
        .map(String::as_str)
        .collect();
    let mut issues = Vec::new();

    for route in routes {
        if config
            .auth_exempt_prefixes
            .iter()
            .any(|prefix| route.path.starts_with(prefix))
        {
            continue;
        }
        if !config
            .auth_required_prefixes
            .iter()
            .any(|prefix| route.path.starts_with(prefix))
        {
            continue;
        }
        if route
            .dependency_names
            .iter()
            .any(|name| required_dependencies.contains(name.as_str()))
        {
            continue;
        }
        issues.push(Issue {
            check: "security/missing-auth-dep",
            severity: "error",
            category: "Security",
            line: 0,
            path: route.path.clone(),
            message: "Protected route is missing required auth Depends()",
            help: "Add a configured auth dependency at the router or handler level so identity comes from request context.",
        });
    }

    issues
}

fn collect_forbidden_write_param_issues(routes: &[RouteRecord], config: &Config) -> Vec<Issue> {
    let write_methods = ["POST", "PUT", "PATCH", "DELETE"];
    let forbidden: HashSet<&str> = config
        .forbidden_write_params
        .iter()
        .map(String::as_str)
        .collect();
    let mut issues = Vec::new();

    for route in routes {
        if !route
            .methods
            .iter()
            .any(|method| write_methods.contains(&method.as_str()))
        {
            continue;
        }
        let mut found = route
            .param_names
            .iter()
            .filter(|param| forbidden.contains(param.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if found.is_empty() {
            continue;
        }
        found.sort();
        found.dedup();
        issues.push(Issue {
            check: "security/forbidden-write-param",
            severity: "error",
            category: "Security",
            line: 0,
            path: route.path.clone(),
            message: Box::leak(
                format!(
                    "Write endpoint accepts forbidden ownership parameters: {}",
                    found.join(", ")
                )
                .into_boxed_str(),
            ),
            help: "Derive identity from auth or request context dependencies instead.",
        });
    }

    issues
}

fn collect_duplicate_route_issues(routes: &[RouteRecord]) -> Vec<Issue> {
    let mut issues = Vec::new();
    let mut seen: HashMap<(&str, &str), &RouteRecord> = HashMap::new();

    for route in routes {
        for method in &route.methods {
            let key = (method.as_str(), route.path.as_str());
            if seen.contains_key(&key) {
                issues.push(Issue {
                    check: "correctness/duplicate-route",
                    severity: "error",
                    category: "Correctness",
                    line: 0,
                    path: route.path.clone(),
                    message: Box::leak(
                        format!("Duplicate route registration for {} {}", method, route.path)
                            .into_boxed_str(),
                    ),
                    help: "Remove the duplicate or use distinct paths.",
                });
            } else {
                seen.insert(key, route);
            }
        }
    }

    issues
}

fn collect_post_status_code_issues(routes: &[RouteRecord], config: &Config) -> Vec<Issue> {
    let mutation_suffixes = ["/undo", "/redo", "/restore", "/refresh", "/clone", "/fork"];
    let mut issues = Vec::new();

    for route in routes {
        if !route.methods.iter().any(|method| method == "POST") {
            continue;
        }
        if route
            .status_code
            .is_some_and(|status_code| status_code != 200)
        {
            continue;
        }
        if mutation_suffixes
            .iter()
            .any(|suffix| route.path.ends_with(suffix))
        {
            continue;
        }
        if config
            .create_post_prefixes
            .iter()
            .any(|prefix| route.path.starts_with(prefix))
        {
            issues.push(Issue {
                check: "correctness/post-status-code",
                severity: "warning",
                category: "Correctness",
                line: 0,
                path: route.path.clone(),
                message: "POST endpoint defaults to 200 — consider 201 for resource creation",
                help: "Add status_code=201 to the route decorator.",
            });
        }
    }

    issues
}

fn collect_missing_response_model_issues(routes: &[RouteRecord]) -> Vec<Issue> {
    let mut issues = Vec::new();

    for route in routes {
        if !route.path.starts_with("/api/") || !route.include_in_schema {
            continue;
        }
        if is_response_model_exempt_path(&route.path) {
            continue;
        }
        if !route.has_response_model {
            issues.push(Issue {
                check: "correctness/missing-response-model",
                severity: "warning",
                category: "Correctness",
                line: 0,
                path: route.path.clone(),
                message:
                    "API endpoint has no response_model — weakens type safety and OpenAPI docs",
                help: "Add response_model=YourPydanticModel to the route decorator.",
            });
        }
    }

    issues
}

fn collect_weak_response_model_issues(routes: &[RouteRecord]) -> Vec<Issue> {
    let mut issues = Vec::new();

    for route in routes {
        if !route.path.starts_with("/api/") || !route.include_in_schema {
            continue;
        }
        if is_response_model_exempt_path(&route.path) {
            continue;
        }
        let Some(response_model) = route.response_model_str.as_deref() else {
            continue;
        };
        let normalized = response_model.replace(' ', "");
        let is_weak = normalized == "dict"
            || normalized == "any"
            || normalized.starts_with("dict[")
            || normalized.starts_with("mapping[")
            || normalized.starts_with("list[dict")
            || normalized.starts_with("list[any]");
        if !is_weak {
            continue;
        }
        issues.push(Issue {
            check: "correctness/weak-response-model",
            severity: "warning",
            category: "Correctness",
            line: 0,
            path: route.path.clone(),
            message: Box::leak(
                format!(
                    "API endpoint uses weak response_model={} — prefer a Pydantic model",
                    response_model
                )
                .into_boxed_str(),
            ),
            help: "Use a concrete BaseModel or typed collection of BaseModels so your API contract stays explicit.",
        });
    }

    issues
}

fn collect_missing_tag_issues(routes: &[RouteRecord], config: &Config) -> Vec<Issue> {
    let mut issues = Vec::new();

    for route in routes {
        if !route.include_in_schema {
            continue;
        }
        if !config
            .tag_required_prefixes
            .iter()
            .any(|prefix| route.path.starts_with(prefix))
        {
            continue;
        }
        if route.tags.is_empty() {
            issues.push(Issue {
                check: "api-surface/missing-tags",
                severity: "warning",
                category: "API Surface",
                line: 0,
                path: route.path.clone(),
                message: "API route is missing tags",
                help: "Add tags=['your-domain'] to the route decorator.",
            });
        }
    }

    issues
}

fn collect_missing_docstring_issues(routes: &[RouteRecord]) -> Vec<Issue> {
    let mut issues = Vec::new();

    for route in routes {
        if !route.path.starts_with("/api/") || !route.include_in_schema || route.has_docstring {
            continue;
        }
        issues.push(Issue {
            check: "api-surface/missing-docstring",
            severity: "warning",
            category: "API Surface",
            line: 0,
            path: route.path.clone(),
            message: Box::leak(
                format!(
                    "Endpoint '{}' has no docstring — weakens API docs",
                    route.endpoint_name
                )
                .into_boxed_str(),
            ),
            help: "Add a docstring to the handler function. FastAPI uses it for OpenAPI descriptions.",
        });
    }

    issues
}

fn collect_missing_pagination_issues(routes: &[RouteRecord]) -> Vec<Issue> {
    let pagination_params = ["limit", "offset", "page", "per_page", "cursor"];
    let exempt_patterns = ["/stream", "/export", "-export", "/download", "/webhook"];
    let mut issues = Vec::new();

    for route in routes {
        if !route.methods.iter().any(|method| method == "GET") || !route.path.starts_with("/api/") {
            continue;
        }
        if exempt_patterns
            .iter()
            .any(|pattern| route.path.contains(pattern))
        {
            continue;
        }
        let Some(response_model_str) = route.response_model_str.as_deref() else {
            continue;
        };
        let looks_like_list =
            response_model_str.contains("list[") || response_model_str.contains("paginated");
        if !looks_like_list {
            continue;
        }
        if route
            .param_names
            .iter()
            .any(|param| pagination_params.contains(&param.as_str()))
        {
            continue;
        }
        issues.push(Issue {
            check: "api-surface/missing-pagination",
            severity: "warning",
            category: "API Surface",
            line: 0,
            path: route.path.clone(),
            message: Box::leak(
                format!(
                    "List endpoint '{}' has no pagination — risks memory exhaustion",
                    route.path
                )
                .into_boxed_str(),
            ),
            help: "Add limit and offset Query parameters to support pagination.",
        });
    }

    issues
}
