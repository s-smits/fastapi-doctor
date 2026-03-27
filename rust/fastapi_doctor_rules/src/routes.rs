use std::collections::{HashMap, HashSet};

use fastapi_doctor_core::{Config, Issue, RouteRecord};

use crate::engine::RuleSelection;

pub(crate) fn analyze_routes(
    routes: &[RouteRecord],
    rules: &RuleSelection,
    config: &Config,
) -> Vec<Issue> {
    let mut issues = Vec::new();

    if rules.forbidden_write_param && !config.forbidden_write_params.is_empty() {
        issues.extend(collect_forbidden_write_param_issues(routes, config));
    }
    if rules.duplicate_route {
        issues.extend(collect_duplicate_route_issues(routes));
    }
    if rules.missing_response_model {
        issues.extend(collect_missing_response_model_issues(routes));
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
    let exempt_patterns = [
        "/stream",
        "/export",
        "-export",
        "/download",
        "/webhook",
        "/oauth",
        "/callback",
        "/{",
    ];
    let mut issues = Vec::new();

    for route in routes {
        if !route.path.starts_with("/api/") || !route.include_in_schema {
            continue;
        }
        if exempt_patterns
            .iter()
            .any(|pattern| route.path.contains(pattern))
        {
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
