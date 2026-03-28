from __future__ import annotations

"""SARIF 2.1.0 output formatter for fastapi-doctor."""

from typing import Any


def _severity_to_sarif_level(severity: str) -> str:
    if severity == "error":
        return "error"
    return "warning"


def _build_rule_descriptors(issues: list[dict[str, Any]]) -> list[dict[str, Any]]:
    seen: dict[str, dict[str, Any]] = {}
    for issue in issues:
        rule_id = issue["check"]
        if rule_id in seen:
            continue
        seen[rule_id] = {
            "id": rule_id,
            "shortDescription": {"text": rule_id},
            "defaultConfiguration": {
                "level": _severity_to_sarif_level(issue["severity"]),
            },
        }
        if issue.get("help"):
            seen[rule_id]["helpUri"] = None
            seen[rule_id]["help"] = {"text": issue["help"]}
    return list(seen.values())


def _build_results(issues: list[dict[str, Any]]) -> list[dict[str, Any]]:
    results = []
    for issue in issues:
        result: dict[str, Any] = {
            "ruleId": issue["check"],
            "level": _severity_to_sarif_level(issue["severity"]),
            "message": {"text": issue["message"]},
        }
        if issue.get("path"):
            location: dict[str, Any] = {
                "physicalLocation": {
                    "artifactLocation": {
                        "uri": issue["path"],
                        "uriBaseId": "%SRCROOT%",
                    },
                }
            }
            if issue.get("line"):
                location["physicalLocation"]["region"] = {
                    "startLine": issue["line"],
                }
            result["locations"] = [location]
        results.append(result)
    return results


def to_sarif(
    *,
    issues: list[dict[str, Any]],
    version: str,
) -> dict[str, Any]:
    """Convert doctor issues to a SARIF 2.1.0 log."""
    return {
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [
            {
                "tool": {
                    "driver": {
                        "name": "fastapi-doctor",
                        "version": version,
                        "informationUri": "https://github.com/s-smits/fastapi-doctor",
                        "rules": _build_rule_descriptors(issues),
                    }
                },
                "results": _build_results(issues),
            }
        ],
    }


def to_github_annotations(issues: list[dict[str, Any]]) -> str:
    """Convert doctor issues to GitHub Actions annotation format."""
    lines = []
    for issue in issues:
        level = "error" if issue["severity"] == "error" else "warning"
        path = issue.get("path", "")
        line = issue.get("line", 0)
        message = issue["message"]
        title = issue["check"]
        lines.append(f"::{level} file={path},line={line},title={title}::{message}")
    return "\n".join(lines)
