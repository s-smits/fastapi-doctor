from __future__ import annotations

"""External command helpers used by the CLI."""

import json
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .models import DoctorIssue


@dataclass(slots=True)
class CommandResult:
    name: str
    command: list[str]
    returncode: int
    stdout: str
    stderr: str
    failure_reason: str | None = None

    @property
    def passed(self) -> bool:
        return self.returncode == 0

    def to_dict(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "command": self.command,
            "returncode": self.returncode,
            "passed": self.passed,
            "status": "passed" if self.passed else (self.failure_reason or "failed"),
            "failure_reason": self.failure_reason,
            "stdout": self.stdout,
            "stderr": self.stderr,
        }


def run_command(name: str, command: list[str], cwd: Path) -> CommandResult:
    try:
        proc = subprocess.run(
            command,
            cwd=cwd,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError as exc:
        return CommandResult(
            name=name,
            command=command,
            returncode=127,
            stdout="",
            stderr=str(exc),
            failure_reason="command-not-found",
        )
    return CommandResult(
        name=name,
        command=command,
        returncode=proc.returncode,
        stdout=proc.stdout,
        stderr=proc.stderr,
    )


def count_bandit_highs(stdout: str) -> int:
    """Bandit output: 'Severity: High' counts."""
    return stdout.count("Severity: High")


def parse_ruff_json(stdout: str) -> list[dict[str, Any]]:
    try:
        payload = json.loads(stdout)
    except json.JSONDecodeError:
        return []
    return payload if isinstance(payload, list) else []


def map_ruff_findings_to_doctor(stdout: str) -> list[DoctorIssue]:
    findings = parse_ruff_json(stdout)
    issues: list[DoctorIssue] = []
    for finding in findings:
        if not isinstance(finding, dict):
            continue
        code = finding.get("code")
        filename = str(finding.get("filename", ""))
        location = finding.get("location") or {}
        row = int(location.get("row", 0) or 0)

        if code == "T201":
            issues.append(
                DoctorIssue(
                    check="architecture/print-in-production",
                    severity="warning",
                    message="print() in production code — use logger instead",
                    path=filename,
                    category="Architecture",
                    help="Replace with logger.info/debug/warning as appropriate.",
                    line=row,
                )
            )
        elif code == "F403":
            message = str(finding.get("message", "from module import * — pollutes namespace and breaks static analysis"))
            issues.append(
                DoctorIssue(
                    check="architecture/star-import",
                    severity="warning",
                    message=message,
                    path=filename,
                    category="Architecture",
                    help="Import specific names: from module import Name1, Name2",
                    line=row,
                )
            )
    return issues


__all__ = [
    "CommandResult",
    "count_bandit_highs",
    "map_ruff_findings_to_doctor",
    "parse_ruff_json",
    "run_command",
]
