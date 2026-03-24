from __future__ import annotations

"""Core doctor data models and scoring constants."""

from dataclasses import asdict, dataclass, field
from typing import Any


PERFECT_SCORE = 100
ERROR_RULE_PENALTY = 2.0
WARNING_RULE_PENALTY = 1.0
SCORE_GOOD_THRESHOLD = 80
SCORE_OK_THRESHOLD = 60
SCHEMA_VERSION = "1.1"

_CATEGORY_WHY: dict[str, str] = {
    "Security": "Security findings can expose the service to unauthorized access, data leaks, or unsafe execution paths.",
    "Correctness": "Correctness findings can produce wrong results, broken endpoints, or runtime failures.",
    "Architecture": "Architecture findings make the codebase harder for agents and humans to modify safely.",
    "API Surface": "API surface findings degrade OpenAPI quality, downstream code generation, or endpoint consistency.",
    "Pydantic": "Schema findings weaken validation at trust boundaries and make request or response handling less reliable.",
    "Resilience": "Resilience findings make failures harder to diagnose and recovery paths less reliable.",
    "Config": "Configuration findings bypass typed settings and make deployments less predictable.",
    "Performance": "Performance findings highlight code patterns likely to waste queries, CPU time, or event-loop capacity.",
    "Doctor": "Internal diagnostic — a doctor check itself encountered an issue that affects report completeness.",
}

# ── Rule metadata: kind ──────────────────────────────────────────────────────
# ``kind`` classifies findings by real-world impact:
#   blocker    — runtime/security failures that must be fixed before shipping
#   risk       — likely to cause problems but not guaranteed
#   opinionated — architecture/style preferences
#   hygiene    — low-priority cleanup
#
# Defaults are derived from (category, severity).  Overrides for specific rules:
_KIND_OVERRIDES: dict[str, str] = {
    # Demote from default blocker → risk (error severity, but lower real impact)
    "security/assert-in-production": "risk",  # -O flag rarely used in production
    # Promote from default opinionated/hygiene → risk (real bug potential)
    "architecture/async-without-await": "risk",
    # Demote from default risk → opinionated (stylistic preference)
    "correctness/avoid-os-path": "opinionated",
    "correctness/deprecated-typing-imports": "opinionated",
    "correctness/serverless-filesystem-write": "opinionated",
    # Doctor internal diagnostics — bootstrap failure is a blocker
    "doctor/app-bootstrap-failed": "blocker",
}

# ── Rule metadata: confidence ────────────────────────────────────────────────
_CONFIDENCE_OVERRIDES: dict[str, float] = {
    "performance/n-plus-one-hint": 0.4,
    "architecture/passthrough-function": 0.5,
    "correctness/serverless-filesystem-write": 0.5,
    "security/hardcoded-secret": 0.8,
}

# ── Rule metadata: recommended action type ───────────────────────────────────
_ACTION_TYPE_OVERRIDES: dict[str, str] = {
    "security/cors-wildcard": "config_tune",
    "config/direct-env-access": "config_tune",
    "config/alembic-target-metadata": "config_tune",
    "config/alembic-empty-autogen-revision": "config_tune",
    "config/sqlalchemy-naming-convention": "config_tune",
    "architecture/giant-function": "review_manually",
    "architecture/god-module": "review_manually",
    "architecture/deep-nesting": "review_manually",
    "architecture/fat-route-handler": "review_manually",
}

# ── Profile tier: fix priority across profiles ───────────────────────────────
# Lower tier = fix first.  Mirrors the rule sets in runner.py (keep in sync).
# Tier 0 (security) — always most urgent
# Tier 1 (balanced)  — stability / correctness second
# Tier 2 (strict)   — style / optimization last
_SECURITY_TIER_SELECTORS = frozenset({
    "security/",
    "pydantic/sensitive-field-type",
    "pydantic/extra-allow-on-request",
    "config/direct-env-access",
})
_BALANCED_TIER_SELECTORS = frozenset({
    "correctness/",
    "resilience/",
    "config/",
    "pydantic/mutable-default",
    "pydantic/deprecated-validator",
    "architecture/async-without-await",
    "architecture/avoid-sys-exit",
    "architecture/engine-pool-pre-ping",
    "architecture/missing-startup-validation",
    "architecture/passthrough-function",
    "architecture/print-in-production",
    "api-surface/missing-pagination",
    "api-surface/missing-operation-id",
    "api-surface/duplicate-operation-id",
    "api-surface/missing-openapi-tags",
})
_PROFILE_TIER_LABELS = {0: "security", 1: "balanced", 2: "strict"}


def _resolve_profile_tier(rule_id: str) -> int:
    """Return the profile tier for a rule (0=security, 1=balanced, 2=strict)."""
    for sel in _SECURITY_TIER_SELECTORS:
        if sel.endswith("/") and rule_id.startswith(sel):
            return 0
        if rule_id == sel:
            return 0
    for sel in _BALANCED_TIER_SELECTORS:
        if sel.endswith("/") and rule_id.startswith(sel):
            return 1
        if rule_id == sel:
            return 1
    return 2

_KIND_ORDER: dict[str, int] = {"blocker": 0, "risk": 1, "opinionated": 2, "hygiene": 3}
_DEFAULT_CONFIDENCE: dict[str, float] = {
    "blocker": 0.95,
    "risk": 0.8,
    "opinionated": 0.7,
    "hygiene": 0.6,
}


@dataclass(slots=True)
class DoctorIssue:
    """Single diagnostic emitted by a check."""

    check: str  # rule identifier, e.g. "security/missing-auth-dep"
    severity: str  # "error" or "warning"
    message: str
    path: str  # route path or file path
    category: str = "Other"
    help: str = ""
    detail: str | None = None
    methods: tuple[str, ...] = ()
    line: int = 0
    column: int = 0

    # ── Derived properties ────────────────────────────────────────────────

    @property
    def blocking(self) -> bool:
        return self.severity == "error"

    @property
    def kind(self) -> str:
        """Classify finding as blocker / risk / opinionated / hygiene."""
        if self.check in _KIND_OVERRIDES:
            return _KIND_OVERRIDES[self.check]
        if self.category in ("Security", "Correctness") and self.severity == "error":
            return "blocker"
        if self.category in ("Security", "Correctness", "Resilience"):
            return "risk"
        if self.category in ("Architecture", "Pydantic"):
            return "opinionated"
        return "hygiene"

    @property
    def priority(self) -> str:
        k = self.kind
        if k == "blocker":
            return "high"
        if k == "risk":
            return "medium"
        return "low"

    @property
    def confidence(self) -> float:
        if self.check in _CONFIDENCE_OVERRIDES:
            return _CONFIDENCE_OVERRIDES[self.check]
        return _DEFAULT_CONFIDENCE.get(self.kind, 0.7)

    @property
    def action_type(self) -> str:
        """Recommended action: code_fix / config_tune / suppress_with_reason / review_manually."""
        if self.check in _ACTION_TYPE_OVERRIDES:
            return _ACTION_TYPE_OVERRIDES[self.check]
        if self.kind in ("blocker", "risk"):
            return "code_fix"
        return "review_manually"

    @property
    def is_ship_blocker(self) -> bool:
        return self.kind == "blocker"

    @property
    def profile_tier(self) -> int:
        """Profile tier: 0=security, 1=balanced, 2=strict."""
        return _resolve_profile_tier(self.check)

    @property
    def profile_tier_label(self) -> str:
        return _PROFILE_TIER_LABELS[self.profile_tier]

    @property
    def why_it_matters(self) -> str:
        return _CATEGORY_WHY.get(
            self.category,
            "This finding indicates a code-health issue that should be addressed before relying on the result.",
        )

    @property
    def suggested_fix(self) -> str:
        return self.help or self.message

    @property
    def fingerprint(self) -> str:
        return f"{self.check}:{self.path}:{self.line}:{self.column}"

    def to_dict(self) -> dict[str, Any]:
        payload = asdict(self)
        payload.update(
            {
                "blocking": self.blocking,
                "kind": self.kind,
                "priority": self.priority,
                "confidence": self.confidence,
                "action_type": self.action_type,
                "is_ship_blocker": self.is_ship_blocker,
                "profile_tier": self.profile_tier,
                "profile_tier_label": self.profile_tier_label,
                "why_it_matters": self.why_it_matters,
                "suggested_fix": self.suggested_fix,
                "safe_to_autofix": False,
                "fingerprint": self.fingerprint,
            }
        )
        return payload


@dataclass(slots=True)
class DoctorReport:
    """Full diagnostic report with react-doctor-style scoring."""

    route_count: int
    openapi_path_count: int
    issues: list[DoctorIssue]
    score: int = 0
    label: str = ""
    categories: dict[str, int] = field(default_factory=dict)
    checks_not_evaluated: list[str] = field(default_factory=list)
    suppressions: list[dict[str, object]] = field(default_factory=list)

    def __post_init__(self) -> None:
        self._compute_score()

    def _compute_score(self) -> None:
        error_rules: set[str] = set()
        warning_rules: set[str] = set()
        cat_counts: dict[str, int] = {}
        for issue in self.issues:
            if issue.severity == "error":
                error_rules.add(issue.check)
            else:
                warning_rules.add(issue.check)
            cat_counts[issue.category] = cat_counts.get(issue.category, 0) + 1
        penalty = len(error_rules) * ERROR_RULE_PENALTY + len(warning_rules) * WARNING_RULE_PENALTY
        self.score = max(0, round(PERFECT_SCORE - penalty))
        if self.score >= SCORE_GOOD_THRESHOLD:
            self.label = "Great"
        elif self.score >= SCORE_OK_THRESHOLD:
            self.label = "Needs work"
        else:
            self.label = "Critical"
        self.categories = dict(sorted(cat_counts.items()))

    @property
    def error_count(self) -> int:
        return sum(1 for issue in self.issues if issue.severity == "error")

    @property
    def warning_count(self) -> int:
        return sum(1 for issue in self.issues if issue.severity == "warning")

    @property
    def blocker_count(self) -> int:
        """Number of issues classified as ship blockers."""
        return sum(1 for issue in self.issues if issue.is_ship_blocker)

    @property
    def has_ship_blockers(self) -> bool:
        return self.blocker_count > 0

    def rule_counts(self) -> dict[str, int]:
        counts: dict[str, int] = {}
        for issue in self.issues:
            counts[issue.check] = counts.get(issue.check, 0) + 1
        return dict(sorted(counts.items()))

    def next_actions(self) -> list[dict[str, Any]]:
        grouped: dict[str, dict[str, Any]] = {}
        for issue in self.issues:
            entry = grouped.setdefault(
                issue.check,
                {
                    "rule": issue.check,
                    "category": issue.category,
                    "kind": issue.kind,
                    "priority": issue.priority,
                    "blocking": issue.blocking,
                    "confidence": issue.confidence,
                    "action_type": issue.action_type,
                    "profile_tier": issue.profile_tier,
                    "profile_tier_label": issue.profile_tier_label,
                    "occurrences": 0,
                    "summary": issue.message,
                    "why_it_matters": issue.why_it_matters,
                    "suggested_fix": issue.suggested_fix,
                    "sample_paths": [],
                },
            )
            entry["occurrences"] += 1
            if len(entry["sample_paths"]) < 3 and issue.path not in entry["sample_paths"]:
                entry["sample_paths"].append(issue.path)

        return sorted(
            grouped.values(),
            key=lambda item: (
                item["profile_tier"],
                _KIND_ORDER.get(item["kind"], 3),
                -item["occurrences"],
                item["rule"],
            ),
        )

    def to_dict(self) -> dict[str, Any]:
        return {
            "schema_version": SCHEMA_VERSION,
            "score": self.score,
            "label": self.label,
            "route_count": self.route_count,
            "openapi_path_count": self.openapi_path_count,
            "error_count": self.error_count,
            "warning_count": self.warning_count,
            "blocker_count": self.blocker_count,
            "has_ship_blockers": self.has_ship_blockers,
            "categories": self.categories,
            "checks_not_evaluated": self.checks_not_evaluated,
            "suppressions": self.suppressions,
            "rule_counts": self.rule_counts(),
            "next_actions": self.next_actions(),
            "issues": [issue.to_dict() for issue in self.issues],
        }


__all__ = [
    "DoctorIssue",
    "DoctorReport",
    "ERROR_RULE_PENALTY",
    "PERFECT_SCORE",
    "SCHEMA_VERSION",
    "SCORE_GOOD_THRESHOLD",
    "SCORE_OK_THRESHOLD",
    "WARNING_RULE_PENALTY",
]
