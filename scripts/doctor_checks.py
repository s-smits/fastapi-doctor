#!/usr/bin/env python3
from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "src"
if str(SRC) not in sys.path:
    sys.path.insert(0, str(SRC))

from python_doctor.app_loader import build_app_for_doctor, dependency_names, iter_api_routes
from python_doctor.models import (
    DoctorIssue,
    DoctorReport,
    ERROR_RULE_PENALTY,
    PERFECT_SCORE,
    SCORE_GOOD_THRESHOLD,
    SCORE_OK_THRESHOLD,
    WARNING_RULE_PENALTY,
)
from python_doctor.project import (
    APP_MODULE,
    ARCHITECTURE_ENABLED,
    ASYNC_ENDPOINT_NOAWAIT_EXCLUDE,
    DEFAULT_ENV,
    FORBIDDEN_WRITE_PARAMS,
    GIANT_FUNCTION_THRESHOLD,
    GOD_MODULE_THRESHOLD,
    IMPORT_ROOT,
    LARGE_FUNCTION_THRESHOLD,
    OWN_CODE_DIR,
    PROTECTED_ROUTE_RULES,
    ProjectLayout,
    REPO_ROOT,
    SHOULD_BE_MODEL_MODE,
    discover_libraries,
    get_project_layout,
    own_python_files,
    refresh_runtime_config,
)
from python_doctor.runner import run_python_doctor_checks


__all__ = [
    "APP_MODULE",
    "ARCHITECTURE_ENABLED",
    "ASYNC_ENDPOINT_NOAWAIT_EXCLUDE",
    "DEFAULT_ENV",
    "DoctorIssue",
    "DoctorReport",
    "ERROR_RULE_PENALTY",
    "FORBIDDEN_WRITE_PARAMS",
    "GIANT_FUNCTION_THRESHOLD",
    "GOD_MODULE_THRESHOLD",
    "IMPORT_ROOT",
    "LARGE_FUNCTION_THRESHOLD",
    "OWN_CODE_DIR",
    "PERFECT_SCORE",
    "PROTECTED_ROUTE_RULES",
    "ProjectLayout",
    "REPO_ROOT",
    "SCORE_GOOD_THRESHOLD",
    "SCORE_OK_THRESHOLD",
    "SHOULD_BE_MODEL_MODE",
    "WARNING_RULE_PENALTY",
    "build_app_for_doctor",
    "dependency_names",
    "discover_libraries",
    "get_project_layout",
    "iter_api_routes",
    "own_python_files",
    "refresh_runtime_config",
    "run_python_doctor_checks",
]
