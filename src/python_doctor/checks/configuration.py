from __future__ import annotations

"""Configuration hygiene checks."""

import ast
import re

from .. import project
from ..models import DoctorIssue

def check_direct_env_access() -> list[DoctorIssue]:
    """Production code should centralize env reads behind a config/settings layer."""
    issues: list[DoctorIssue] = []
    # Only check router/service code, not config/startup/scripts
    check_dirs = {"routers", "services", "interfaces"}
    # Patterns that are OK (setting defaults, not reading)
    ok_patterns = re.compile(r"os\.environ\.setdefault|os\.environ\[.+\]\s*=|os\.environ\.get\(.+,")

    for filepath in project.own_python_files():
        parts = filepath.relative_to(project.OWN_CODE_DIR).parts
        if not parts or parts[0] not in check_dirs:
            continue
        try:
            lines = filepath.read_text().splitlines()
        except Exception:
            continue
        for i, line in enumerate(lines, 1):
            stripped = line.strip()
            if "os.environ" in stripped and not ok_patterns.search(stripped):
                if "# noqa: direct-env" in stripped:
                    continue
                # Check for direct reads like os.environ["KEY"] or os.environ.get("KEY")
                if re.search(r"os\.environ\s*\[", stripped) or re.search(r"os\.environ\.get\(", stripped):
                    issues.append(
                        DoctorIssue(
                            check="config/direct-env-access",
                            severity="warning",
                            message="Direct os.environ access in service/router code — use settings object",
                            path=str(filepath.relative_to(project.REPO_ROOT)),
                            category="Config",
                            help="Read env vars in one config/settings module, then inject the typed setting where needed.",
                            line=i,
                        )
                    )
    return issues


__all__ = ["check_direct_env_access"]
