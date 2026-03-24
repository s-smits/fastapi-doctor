from __future__ import annotations

"""Structured suppression support for fastapi-doctor.

Two suppression syntaxes are recognised:

1. ``# doctor:ignore <rule> reason="..."``  — structured, with audit trail
2. ``# noqa[: <codes>]``                    — legacy, backward-compatible

The ``doctor:ignore`` form is preferred: it preserves intent, shows up in the
JSON ``suppressions`` array, and lets reviewers verify that each suppression
is still warranted.
"""

import re

# # doctor:ignore security/hardcoded-secret reason="enum label, not a secret"
_DOCTOR_IGNORE_RE = re.compile(
    r"#\s*doctor:ignore\s+([\w/*-]+)(?:\s+reason=\"([^\"]*)\")?",
    re.IGNORECASE,
)

# # noqa   OR   # noqa: sql-safe, security
_NOQA_RE = re.compile(r"#\s*noqa(?:\s*:\s*([\w\-,\s/]+))?", re.IGNORECASE)

# Legacy noqa code aliases kept for backward compat.
_NOQA_ALIASES: dict[str, str] = {
    "sql-safe": "security/",
    "security": "security/",
    "architecture": "architecture/",
    "direct-env": "config/direct-env-access",
}


def _selector_matches(rule_id: str, selector: str) -> bool:
    selector = selector.strip()
    if selector.endswith("*"):
        return rule_id.startswith(selector[:-1])
    if selector.endswith("/"):
        return rule_id.startswith(selector)
    return rule_id == selector


def is_suppressed(line: str, rule_id: str) -> bool:
    """Return True if *line* contains a suppression comment matching *rule_id*.

    Recognises both ``# doctor:ignore <rule>`` and ``# noqa[: <codes>]``.
    """
    # 1. Structured doctor:ignore
    m = _DOCTOR_IGNORE_RE.search(line)
    if m and _selector_matches(rule_id, m.group(1)):
        return True

    # 2. Legacy # noqa
    m = _NOQA_RE.search(line)
    if m:
        codes = m.group(1)
        if codes is None:
            return True  # bare ``# noqa`` suppresses everything
        for code in codes.split(","):
            code = code.strip()
            if not code:
                continue
            if _selector_matches(rule_id, code):
                return True
            alias_target = _NOQA_ALIASES.get(code)
            if alias_target and _selector_matches(rule_id, alias_target):
                return True
    return False


def collect_suppressions(source: str, file_path: str) -> list[dict[str, object]]:
    """Return all ``# doctor:ignore`` comments found in *source*."""
    results: list[dict[str, object]] = []
    for i, line in enumerate(source.splitlines(), 1):
        m = _DOCTOR_IGNORE_RE.search(line)
        if m:
            results.append(
                {
                    "rule": m.group(1).strip(),
                    "reason": m.group(2) or "",
                    "path": file_path,
                    "line": i,
                }
            )
    return results


__all__ = ["collect_suppressions", "is_suppressed"]
