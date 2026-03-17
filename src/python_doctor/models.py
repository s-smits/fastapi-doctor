from __future__ import annotations

"""Core doctor data models and scoring constants."""

from dataclasses import asdict, dataclass, field
from typing import Any


PERFECT_SCORE = 100
ERROR_RULE_PENALTY = 2.0
WARNING_RULE_PENALTY = 1.0
SCORE_GOOD_THRESHOLD = 80
SCORE_OK_THRESHOLD = 60


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

    def to_dict(self) -> dict[str, Any]:
        return {
            "score": self.score,
            "label": self.label,
            "route_count": self.route_count,
            "openapi_path_count": self.openapi_path_count,
            "error_count": self.error_count,
            "warning_count": self.warning_count,
            "categories": self.categories,
            "issues": [asdict(issue) for issue in self.issues],
        }


__all__ = [
    "DoctorIssue",
    "DoctorReport",
    "ERROR_RULE_PENALTY",
    "PERFECT_SCORE",
    "SCORE_GOOD_THRESHOLD",
    "SCORE_OK_THRESHOLD",
    "WARNING_RULE_PENALTY",
]
