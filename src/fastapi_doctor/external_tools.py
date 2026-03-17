from __future__ import annotations

"""External command helpers used by the CLI."""

import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any


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

__all__ = ["CommandResult", "count_bandit_highs", "run_command"]
