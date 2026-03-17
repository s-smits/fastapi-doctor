from __future__ import annotations

"""Core doctor data models and scoring constants."""

from dataclasses import asdict, dataclass, field
from typing import Any


PERFECT_SCORE = 100
ERROR_RULE_PENALTY = 2.0
WARNING_RULE_PENALTY = 1.0
SCORE_GOOD_THRESHOLD = 80
SCORE_OK_THRESHOLD = 60
SCHEMA_VERSION = "1.0"

_CATEGORY_WHY: dict[str, str] = {
    "Security": "Security findings can expose the service to unauthorized access, data leaks, or unsafe execution paths.",
    "Correctness": "Correctness findings can produce wrong results, broken endpoints, or runtime failures.",
    "Architecture": "Architecture findings make the codebase harder for agents and humans to modify safely.",
    "API Surface": "API surface findings degrade OpenAPI quality, downstream code generation, or endpoint consistency.",
    "Pydantic": "Schema findings weaken validation at trust boundaries and make request or response handling less reliable.",
    "Resilience": "Resilience findings make failures harder to diagnose and recovery paths less reliable.",
    "Config": "Configuration findings bypass typed settings and make deployments less predictable.",
    "Performance": "Performance findings highlight code patterns likely to waste queries, CPU time, or event-loop capacity.",
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

    @property
    def blocking(self) -> bool:
        return self.severity == "error"

    @property
    def priority(self) -> str:
        return "high" if self.blocking else "medium"

    @property
    def confidence(self) -> float:
        return 0.9 if self.blocking else 0.75

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
                "priority": self.priority,
                "confidence": self.confidence,
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
                    "priority": issue.priority,
                    "blocking": issue.blocking,
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
                0 if item["blocking"] else 1,
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
            "categories": self.categories,
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
