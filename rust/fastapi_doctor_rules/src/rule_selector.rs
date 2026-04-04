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
    "correctness/serverless-filesystem-write",
    "correctness/missing-http-timeout",
    "resilience/*",
    "pydantic/mutable-default",
    "pydantic/deprecated-validator",
    "architecture/async-without-await",
    "architecture/avoid-sys-exit",
    "architecture/engine-pool-pre-ping",
    "architecture/missing-startup-validation",
    "architecture/passthrough-function",
    "architecture/print-in-production",
    "api-surface/missing-pagination",
    "api-surface/missing-tags",
    "api-surface/missing-docstring",
];

pub fn parse_static_rule(rule_id: &str) -> Option<StaticRule> {
    Some(match rule_id {
        "architecture/giant-function" => StaticRule::ArchitectureGiantFunction,
        "architecture/giant-route-handler" => StaticRule::ArchitectureGiantRouteHandler,
        "architecture/large-function" => StaticRule::ArchitectureLargeFunction,
        "architecture/deep-nesting" => StaticRule::ArchitectureDeepNesting,
    "architecture/async-without-await" => StaticRule::ArchitectureAsyncWithoutAwait,
        "architecture/import-bloat" => StaticRule::ArchitectureImportBloat,
        "architecture/print-in-production" => StaticRule::ArchitecturePrintInProduction,
        "architecture/star-import" => StaticRule::ArchitectureStarImport,
        "architecture/god-module" => StaticRule::ArchitectureGodModule,
        "architecture/passthrough-function" => StaticRule::ArchitecturePassthroughFunction,
        "architecture/hidden-dependency-instantiation" => {
            StaticRule::ArchitectureHiddenDependencyInstantiation
        }
        "architecture/flag-argument-dispatch" => StaticRule::ArchitectureFlagArgumentDispatch,
    "architecture/avoid-sys-exit" => StaticRule::ArchitectureAvoidSysExit,
    "architecture/fat-route-handler" => StaticRule::ArchitectureFatRouteHandler,
        "security/missing-auth-dep" => StaticRule::SecurityMissingAuthDep,
        "security/forbidden-write-param" => StaticRule::SecurityForbiddenWriteParam,
        "correctness/duplicate-route" => StaticRule::CorrectnessDuplicateRoute,
        "correctness/missing-response-model" => StaticRule::CorrectnessMissingResponseModel,
        "correctness/weak-response-model" => StaticRule::CorrectnessWeakResponseModel,
        "correctness/post-status-code" => StaticRule::CorrectnessPostStatusCode,
        "api-surface/missing-tags" => StaticRule::ApiSurfaceMissingTags,
        "api-surface/missing-docstring" => StaticRule::ApiSurfaceMissingDocstring,
        "api-surface/missing-pagination" => StaticRule::ApiSurfaceMissingPagination,
        "config/direct-env-access" => StaticRule::ConfigDirectEnvAccess,
        "config/env-mutation" => StaticRule::ConfigEnvMutation,
        "config/alembic-target-metadata" => StaticRule::ConfigAlembicTargetMetadata,
        "config/alembic-empty-autogen-revision" => StaticRule::ConfigAlembicEmptyAutogenRevision,
        "config/sqlalchemy-naming-convention" => StaticRule::ConfigSqlalchemyNamingConvention,
        "correctness/asyncio-run-in-async" => StaticRule::CorrectnessAsyncioRunInAsync,
        "correctness/sync-io-in-async" => StaticRule::CorrectnessSyncIoInAsync,
        "correctness/misused-async-constructs" => StaticRule::CorrectnessMisusedAsyncConstructs,
        "correctness/avoid-os-path" => StaticRule::CorrectnessAvoidOsPath,
        "correctness/deprecated-typing-imports" => StaticRule::CorrectnessDeprecatedTypingImports,
        "correctness/mutable-default-arg" => StaticRule::CorrectnessMutableDefaultArg,
        "correctness/import-time-default-call" => StaticRule::CorrectnessImportTimeDefaultCall,
        "correctness/naive-datetime" => StaticRule::CorrectnessNaiveDatetime,
        "correctness/return-in-finally" => StaticRule::CorrectnessReturnInFinally,
        "correctness/threading-lock-in-async" => StaticRule::CorrectnessThreadingLockInAsync,
        "correctness/unreachable-code" => StaticRule::CorrectnessUnreachableCode,
        "correctness/get-with-side-effect" => StaticRule::CorrectnessGetWithSideEffect,
    "correctness/exposed-mutable-state" => StaticRule::CorrectnessExposedMutableState,
    "correctness/serverless-filesystem-write" => {
            StaticRule::CorrectnessServerlessFilesystemWrite
        }
    "correctness/missing-http-timeout" => StaticRule::CorrectnessMissingHttpTimeout,
    "performance/sequential-awaits" => StaticRule::PerformanceSequentialAwaits,
    "performance/regex-in-loop" => StaticRule::PerformanceRegexInLoop,
    "performance/n-plus-one-hint" => StaticRule::PerformanceNPlusOneHint,
        "pydantic/deprecated-validator" => StaticRule::PydanticDeprecatedValidator,
        "pydantic/mutable-default" => StaticRule::PydanticMutableDefault,
        "pydantic/extra-allow-on-request" => StaticRule::PydanticExtraAllowOnRequest,
        "pydantic/should-be-model" => StaticRule::PydanticShouldBeModel,
        "pydantic/sensitive-field-type" => StaticRule::PydanticSensitiveFieldType,
        "pydantic/normalized-name-collision" => StaticRule::PydanticNormalizedNameCollision,
        "security/assert-in-production" => StaticRule::SecurityAssertInProduction,
        "security/cors-wildcard" => StaticRule::SecurityCorsWildcard,
        "security/exception-detail-leak" => StaticRule::SecurityExceptionDetailLeak,
        "security/subprocess-shell-true" => StaticRule::SecuritySubprocessShellTrue,
        "security/unsafe-yaml-load" => StaticRule::SecurityUnsafeYamlLoad,
        "security/weak-hash-without-flag" => StaticRule::SecurityWeakHashWithoutFlag,
        "security/sql-fstring-interpolation" => StaticRule::SecuritySqlFstringInterpolation,
        "security/hardcoded-secret" => StaticRule::SecurityHardcodedSecret,
        "security/pydantic-secretstr" => StaticRule::SecurityPydanticSecretStr,
        "resilience/sqlalchemy-pool-pre-ping" => StaticRule::ResilienceSqlalchemyPoolPrePing,
        "resilience/bare-except-pass" => StaticRule::ResilienceBareExceptPass,
        "resilience/reraise-without-context" => StaticRule::ResilienceReraiseWithoutContext,
        "resilience/exception-swallowed" => StaticRule::ResilienceExceptionSwallowed,
        "resilience/broad-except-no-context" => StaticRule::ResilienceBroadExceptNoContext,
        "resilience/exception-log-without-traceback" => {
            StaticRule::ResilienceExceptionLogWithoutTraceback
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_static_rule, MEDIUM_SELECTORS, SECURITY_SELECTORS};

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

    if let Some(profile) = profile {
        match profile {
            "security" => {
                if !SECURITY_SELECTORS
                    .iter()
                    .any(|selector| selector_matches(rule_id, selector))
                {
                    return false;
                }
            }
            "medium" | "balanced" => {
                if !MEDIUM_SELECTORS
                    .iter()
                    .any(|selector| selector_matches(rule_id, selector))
                {
                    return false;
                }
            }
            _ => {}
        }
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
