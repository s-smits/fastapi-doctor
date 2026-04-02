#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StaticRule {
    ArchitectureGiantFunction,
    ArchitectureGiantRouteHandler,
    ArchitectureLargeFunction,
    ArchitectureDeepNesting,
    ArchitectureAsyncWithoutAwait,
    ArchitectureImportBloat,
    ArchitecturePrintInProduction,
    ArchitectureStarImport,
    ArchitectureGodModule,
    ArchitecturePassthroughFunction,
    ArchitectureAvoidSysExit,
    ArchitectureEnginePoolPrePing,
    ArchitectureMissingStartupValidation,
    ArchitectureFatRouteHandler,
    SecurityMissingAuthDep,
    SecurityForbiddenWriteParam,
    CorrectnessDuplicateRoute,
    CorrectnessMissingResponseModel,
    CorrectnessWeakResponseModel,
    CorrectnessPostStatusCode,
    ApiSurfaceMissingTags,
    ApiSurfaceMissingDocstring,
    ApiSurfaceMissingPagination,
    ConfigDirectEnvAccess,
    ConfigEnvMutation,
    ConfigAlembicTargetMetadata,
    ConfigAlembicEmptyAutogenRevision,
    ConfigSqlalchemyNamingConvention,
    CorrectnessAsyncioRunInAsync,
    CorrectnessSyncIoInAsync,
    CorrectnessMisusedAsyncConstructs,
    CorrectnessAvoidOsPath,
    CorrectnessDeprecatedTypingImports,
    CorrectnessMutableDefaultArg,
    CorrectnessNaiveDatetime,
    CorrectnessReturnInFinally,
    CorrectnessThreadingLockInAsync,
    CorrectnessUnreachableCode,
    CorrectnessGetWithSideEffect,
    CorrectnessServerlessFilesystemWrite,
    CorrectnessMissingHttpTimeout,
    PerformanceHeavyImports,
    PerformanceSequentialAwaits,
    PerformanceRegexInLoop,
    PerformanceNPlusOneHint,
    PydanticDeprecatedValidator,
    PydanticMutableDefault,
    PydanticExtraAllowOnRequest,
    PydanticShouldBeModel,
    PydanticSensitiveFieldType,
    PydanticNormalizedNameCollision,
    SecurityAssertInProduction,
    SecurityCorsWildcard,
    SecurityExceptionDetailLeak,
    SecuritySubprocessShellTrue,
    SecurityUnsafeYamlLoad,
    SecurityWeakHashWithoutFlag,
    SecuritySqlFstringInterpolation,
    SecurityHardcodedSecret,
    SecurityPydanticSecretStr,
    ResilienceSqlalchemyPoolPrePing,
    ResilienceBareExceptPass,
    ResilienceReraiseWithoutContext,
    ResilienceExceptionSwallowed,
    ResilienceBroadExceptNoContext,
    ResilienceExceptionLogWithoutTraceback,
}

impl StaticRule {
    pub fn all() -> &'static [StaticRule] {
        use StaticRule::*;
        &[
            ArchitectureGiantFunction,
            ArchitectureGiantRouteHandler,
            ArchitectureLargeFunction,
            ArchitectureDeepNesting,
            ArchitectureAsyncWithoutAwait,
            ArchitectureImportBloat,
            ArchitecturePrintInProduction,
            ArchitectureStarImport,
            ArchitectureGodModule,
            ArchitecturePassthroughFunction,
            ArchitectureAvoidSysExit,
            ArchitectureEnginePoolPrePing,
            ArchitectureMissingStartupValidation,
            ArchitectureFatRouteHandler,
            SecurityMissingAuthDep,
            SecurityForbiddenWriteParam,
            CorrectnessDuplicateRoute,
            CorrectnessMissingResponseModel,
            CorrectnessWeakResponseModel,
            CorrectnessPostStatusCode,
            ApiSurfaceMissingTags,
            ApiSurfaceMissingDocstring,
            ApiSurfaceMissingPagination,
            ConfigDirectEnvAccess,
            ConfigEnvMutation,
            ConfigAlembicTargetMetadata,
            ConfigAlembicEmptyAutogenRevision,
            ConfigSqlalchemyNamingConvention,
            CorrectnessAsyncioRunInAsync,
            CorrectnessSyncIoInAsync,
            CorrectnessMisusedAsyncConstructs,
            CorrectnessAvoidOsPath,
            CorrectnessDeprecatedTypingImports,
            CorrectnessMutableDefaultArg,
            CorrectnessNaiveDatetime,
            CorrectnessReturnInFinally,
            CorrectnessThreadingLockInAsync,
            CorrectnessUnreachableCode,
            CorrectnessGetWithSideEffect,
            CorrectnessServerlessFilesystemWrite,
            CorrectnessMissingHttpTimeout,
            PerformanceHeavyImports,
            PerformanceSequentialAwaits,
            PerformanceRegexInLoop,
            PerformanceNPlusOneHint,
            PydanticDeprecatedValidator,
            PydanticMutableDefault,
            PydanticExtraAllowOnRequest,
            PydanticShouldBeModel,
            PydanticSensitiveFieldType,
            PydanticNormalizedNameCollision,
            SecurityAssertInProduction,
            SecurityCorsWildcard,
            SecurityExceptionDetailLeak,
            SecuritySubprocessShellTrue,
            SecurityUnsafeYamlLoad,
            SecurityWeakHashWithoutFlag,
            SecuritySqlFstringInterpolation,
            SecurityHardcodedSecret,
            SecurityPydanticSecretStr,
            ResilienceSqlalchemyPoolPrePing,
            ResilienceBareExceptPass,
            ResilienceReraiseWithoutContext,
            ResilienceExceptionSwallowed,
            ResilienceBroadExceptNoContext,
            ResilienceExceptionLogWithoutTraceback,
        ]
    }

    pub const fn rule_id(self) -> &'static str {
        match self {
            Self::ArchitectureGiantFunction => "architecture/giant-function",
            Self::ArchitectureGiantRouteHandler => "architecture/giant-route-handler",
            Self::ArchitectureLargeFunction => "architecture/large-function",
            Self::ArchitectureDeepNesting => "architecture/deep-nesting",
            Self::ArchitectureAsyncWithoutAwait => "architecture/async-without-await",
            Self::ArchitectureImportBloat => "architecture/import-bloat",
            Self::ArchitecturePrintInProduction => "architecture/print-in-production",
            Self::ArchitectureStarImport => "architecture/star-import",
            Self::ArchitectureGodModule => "architecture/god-module",
            Self::ArchitecturePassthroughFunction => "architecture/passthrough-function",
            Self::ArchitectureAvoidSysExit => "architecture/avoid-sys-exit",
            Self::ArchitectureEnginePoolPrePing => "architecture/engine-pool-pre-ping",
            Self::ArchitectureMissingStartupValidation => "architecture/missing-startup-validation",
            Self::ArchitectureFatRouteHandler => "architecture/fat-route-handler",
            Self::SecurityMissingAuthDep => "security/missing-auth-dep",
            Self::SecurityForbiddenWriteParam => "security/forbidden-write-param",
            Self::CorrectnessDuplicateRoute => "correctness/duplicate-route",
            Self::CorrectnessMissingResponseModel => "correctness/missing-response-model",
            Self::CorrectnessWeakResponseModel => "correctness/weak-response-model",
            Self::CorrectnessPostStatusCode => "correctness/post-status-code",
            Self::ApiSurfaceMissingTags => "api-surface/missing-tags",
            Self::ApiSurfaceMissingDocstring => "api-surface/missing-docstring",
            Self::ApiSurfaceMissingPagination => "api-surface/missing-pagination",
            Self::ConfigDirectEnvAccess => "config/direct-env-access",
            Self::ConfigEnvMutation => "config/env-mutation",
            Self::ConfigAlembicTargetMetadata => "config/alembic-target-metadata",
            Self::ConfigAlembicEmptyAutogenRevision => "config/alembic-empty-autogen-revision",
            Self::ConfigSqlalchemyNamingConvention => "config/sqlalchemy-naming-convention",
            Self::CorrectnessAsyncioRunInAsync => "correctness/asyncio-run-in-async",
            Self::CorrectnessSyncIoInAsync => "correctness/sync-io-in-async",
            Self::CorrectnessMisusedAsyncConstructs => "correctness/misused-async-constructs",
            Self::CorrectnessAvoidOsPath => "correctness/avoid-os-path",
            Self::CorrectnessDeprecatedTypingImports => "correctness/deprecated-typing-imports",
            Self::CorrectnessMutableDefaultArg => "correctness/mutable-default-arg",
            Self::CorrectnessNaiveDatetime => "correctness/naive-datetime",
            Self::CorrectnessReturnInFinally => "correctness/return-in-finally",
            Self::CorrectnessThreadingLockInAsync => "correctness/threading-lock-in-async",
            Self::CorrectnessUnreachableCode => "correctness/unreachable-code",
            Self::CorrectnessGetWithSideEffect => "correctness/get-with-side-effect",
            Self::CorrectnessServerlessFilesystemWrite => "correctness/serverless-filesystem-write",
            Self::CorrectnessMissingHttpTimeout => "correctness/missing-http-timeout",
            Self::PerformanceHeavyImports => "performance/heavy-imports",
            Self::PerformanceSequentialAwaits => "performance/sequential-awaits",
            Self::PerformanceRegexInLoop => "performance/regex-in-loop",
            Self::PerformanceNPlusOneHint => "performance/n-plus-one-hint",
            Self::PydanticDeprecatedValidator => "pydantic/deprecated-validator",
            Self::PydanticMutableDefault => "pydantic/mutable-default",
            Self::PydanticExtraAllowOnRequest => "pydantic/extra-allow-on-request",
            Self::PydanticShouldBeModel => "pydantic/should-be-model",
            Self::PydanticSensitiveFieldType => "pydantic/sensitive-field-type",
            Self::PydanticNormalizedNameCollision => "pydantic/normalized-name-collision",
            Self::SecurityAssertInProduction => "security/assert-in-production",
            Self::SecurityCorsWildcard => "security/cors-wildcard",
            Self::SecurityExceptionDetailLeak => "security/exception-detail-leak",
            Self::SecuritySubprocessShellTrue => "security/subprocess-shell-true",
            Self::SecurityUnsafeYamlLoad => "security/unsafe-yaml-load",
            Self::SecurityWeakHashWithoutFlag => "security/weak-hash-without-flag",
            Self::SecuritySqlFstringInterpolation => "security/sql-fstring-interpolation",
            Self::SecurityHardcodedSecret => "security/hardcoded-secret",
            Self::SecurityPydanticSecretStr => "security/pydantic-secretstr",
            Self::ResilienceSqlalchemyPoolPrePing => "resilience/sqlalchemy-pool-pre-ping",
            Self::ResilienceBareExceptPass => "resilience/bare-except-pass",
            Self::ResilienceReraiseWithoutContext => "resilience/reraise-without-context",
            Self::ResilienceExceptionSwallowed => "resilience/exception-swallowed",
            Self::ResilienceBroadExceptNoContext => "resilience/broad-except-no-context",
            Self::ResilienceExceptionLogWithoutTraceback => {
                "resilience/exception-log-without-traceback"
            }
        }
    }

    pub const fn severity(self) -> &'static str {
        match self {
            Self::SecurityMissingAuthDep
            | Self::ArchitectureGiantRouteHandler
            | Self::SecurityForbiddenWriteParam
            | Self::SecurityAssertInProduction
            | Self::SecuritySubprocessShellTrue
            | Self::SecurityUnsafeYamlLoad
            | Self::SecuritySqlFstringInterpolation
            | Self::SecurityHardcodedSecret
            | Self::SecurityPydanticSecretStr => "error",
            _ => "warning",
        }
    }

    pub const fn category(self) -> &'static str {
        match self {
            Self::ArchitectureGiantFunction
            | Self::ArchitectureGiantRouteHandler
            | Self::ArchitectureLargeFunction
            | Self::ArchitectureDeepNesting
            | Self::ArchitectureAsyncWithoutAwait
            | Self::ArchitectureImportBloat
            | Self::ArchitecturePrintInProduction
            | Self::ArchitectureStarImport
            | Self::ArchitectureGodModule
            | Self::ArchitecturePassthroughFunction
            | Self::ArchitectureAvoidSysExit
            | Self::ArchitectureEnginePoolPrePing
            | Self::ArchitectureMissingStartupValidation
            | Self::ArchitectureFatRouteHandler => "Architecture",

            Self::SecurityMissingAuthDep
            | Self::SecurityForbiddenWriteParam
            | Self::SecurityAssertInProduction
            | Self::SecurityCorsWildcard
            | Self::SecurityExceptionDetailLeak
            | Self::SecuritySubprocessShellTrue
            | Self::SecurityUnsafeYamlLoad
            | Self::SecurityWeakHashWithoutFlag
            | Self::SecuritySqlFstringInterpolation
            | Self::SecurityHardcodedSecret
            | Self::SecurityPydanticSecretStr => "Security",

            Self::CorrectnessDuplicateRoute
            | Self::CorrectnessMissingResponseModel
            | Self::CorrectnessWeakResponseModel
            | Self::CorrectnessPostStatusCode
            | Self::CorrectnessAsyncioRunInAsync
            | Self::CorrectnessSyncIoInAsync
            | Self::CorrectnessMisusedAsyncConstructs
            | Self::CorrectnessAvoidOsPath
            | Self::CorrectnessDeprecatedTypingImports
            | Self::CorrectnessMutableDefaultArg
            | Self::CorrectnessNaiveDatetime
            | Self::CorrectnessReturnInFinally
            | Self::CorrectnessThreadingLockInAsync
            | Self::CorrectnessUnreachableCode
            | Self::CorrectnessGetWithSideEffect
            | Self::CorrectnessServerlessFilesystemWrite
            | Self::CorrectnessMissingHttpTimeout => "Correctness",

            Self::ApiSurfaceMissingTags
            | Self::ApiSurfaceMissingDocstring
            | Self::ApiSurfaceMissingPagination => "API Surface",

            Self::ConfigDirectEnvAccess
            | Self::ConfigEnvMutation
            | Self::ConfigAlembicTargetMetadata
            | Self::ConfigAlembicEmptyAutogenRevision
            | Self::ConfigSqlalchemyNamingConvention => "Configuration",

            Self::PerformanceHeavyImports
            | Self::PerformanceSequentialAwaits
            | Self::PerformanceRegexInLoop
            | Self::PerformanceNPlusOneHint => "Performance",

            Self::PydanticDeprecatedValidator
            | Self::PydanticMutableDefault
            | Self::PydanticExtraAllowOnRequest
            | Self::PydanticShouldBeModel
            | Self::PydanticSensitiveFieldType
            | Self::PydanticNormalizedNameCollision => "Pydantic",

            Self::ResilienceSqlalchemyPoolPrePing
            | Self::ResilienceBareExceptPass
            | Self::ResilienceReraiseWithoutContext
            | Self::ResilienceExceptionSwallowed
            | Self::ResilienceBroadExceptNoContext
            | Self::ResilienceExceptionLogWithoutTraceback => "Resilience",
        }
    }
}
