use crate::registry::StaticRule;

const SECURITY_SELECTORS: &[&str] = &[
    "security/*",
    "pydantic/sensitive-field-type",
    "pydantic/extra-allow-on-request",
    "config/direct-env-access",
];

const MEDIUM_SELECTORS: &[&str] = &[
    "security/*",
    "pydantic/sensitive-field-type",
    "pydantic/extra-allow-on-request",
    "pydantic/normalized-name-collision",
    "config/direct-env-access",
    "config/alembic-target-metadata",
    "config/alembic-empty-autogen-revision",
    "config/sqlalchemy-naming-convention",
    "correctness/duplicate-route",
    "correctness/missing-response-model",
    "correctness/weak-response-model",
    "correctness/post-status-code",
    "correctness/asyncio-run-in-async",
    "correctness/sync-io-in-async",
    "correctness/misused-async-constructs",
    "correctness/avoid-os-path",
    "correctness/deprecated-typing-imports",
    "correctness/mutable-default-arg",
    "correctness/import-time-default-call",
    "correctness/naive-datetime",
    "correctness/return-in-finally",
    "correctness/threading-lock-in-async",
    "correctness/unreachable-code",
    "correctness/get-with-side-effect",
    "correctness/untracked-background-task",
    "correctness/serverless-filesystem-write",
    "correctness/missing-http-timeout",
    "resilience/*",
    "pydantic/mutable-default",
    "pydantic/deprecated-validator",
    "architecture/async-without-await",
    "architecture/avoid-sys-exit",
    "architecture/httpexception-in-service",
    "architecture/missing-startup-validation",
    "architecture/passthrough-function",
    "architecture/print-in-production",
    "api-surface/missing-pagination",
    "api-surface/missing-tags",
    "api-surface/missing-docstring",
];

pub fn parse_static_rule(rule_id: &str) -> Option<StaticRule> {
    StaticRule::all()
        .iter()
        .copied()
        .find(|rule| rule.rule_id() == rule_id)
}

pub fn select_rule_ids(
    profile: Option<&str>,
    only_rules: &[String],
    ignore_rules: &[String],
    exclude_rules: &[String],
    skip_structure: bool,
    skip_openapi: bool,
) -> Vec<String> {
    StaticRule::all()
        .iter()
        .map(|rule| rule.rule_id())
        .filter(|rule_id| {
            should_run(
                rule_id,
                profile,
                only_rules,
                ignore_rules,
                exclude_rules,
                skip_structure,
                skip_openapi,
            )
        })
        .map(str::to_string)
        .collect()
}

fn selector_matches(rule_id: &str, selector: &str) -> bool {
    let selector = selector.trim_end_matches('*');
    rule_id == selector || rule_id.starts_with(selector)
}

fn selectors_for_profile(profile: Option<&str>) -> Option<&'static [&'static str]> {
    match profile {
        Some("security") => Some(SECURITY_SELECTORS),
        Some("medium" | "balanced") => Some(MEDIUM_SELECTORS),
        _ => None,
    }
}

fn should_run(
    rule_id: &str,
    profile: Option<&str>,
    only_rules: &[String],
    ignore_rules: &[String],
    exclude_rules: &[String],
    skip_structure: bool,
    skip_openapi: bool,
) -> bool {
    if !only_rules.is_empty() {
        return only_rules
            .iter()
            .any(|selector| selector_matches(rule_id, selector));
    }

    if selectors_for_profile(profile).is_some_and(|selectors| {
        !selectors
            .iter()
            .any(|selector| selector_matches(rule_id, selector))
    }) {
        return false;
    }

    if skip_structure
        && [
            "architecture/",
            "correctness/",
            "pydantic/",
            "resilience/",
            "security/",
            "config/",
        ]
        .iter()
        .any(|selector| selector_matches(rule_id, selector))
    {
        return false;
    }

    if skip_openapi && selector_matches(rule_id, "api-surface/") {
        return false;
    }

    if ignore_rules
        .iter()
        .any(|selector| selector_matches(rule_id, selector))
    {
        return false;
    }

    if exclude_rules
        .iter()
        .any(|selector| selector_matches(rule_id, selector))
    {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{parse_static_rule, MEDIUM_SELECTORS, SECURITY_SELECTORS};
    use crate::registry::StaticRule;

    fn assert_exact_selectors_resolve(selectors: &[&str]) {
        for selector in selectors {
            if selector.ends_with('*') {
                continue;
            }
            assert!(
                parse_static_rule(selector).is_some(),
                "selector '{selector}' must resolve to a registered static rule"
            );
        }
    }

    #[test]
    fn security_profile_exact_selectors_resolve() {
        assert_exact_selectors_resolve(SECURITY_SELECTORS);
    }

    #[test]
    fn balanced_profile_exact_selectors_resolve() {
        assert_exact_selectors_resolve(MEDIUM_SELECTORS);
    }

    #[test]
    fn every_registered_rule_round_trips_through_its_id() {
        for rule in StaticRule::all() {
            assert_eq!(parse_static_rule(rule.rule_id()), Some(*rule));
        }
    }

    #[test]
    fn registered_rule_ids_are_unique() {
        let mut seen = HashSet::new();
        for rule in StaticRule::all() {
            assert!(
                seen.insert(rule.rule_id()),
                "duplicate rule id: {}",
                rule.rule_id()
            );
        }
    }
}
