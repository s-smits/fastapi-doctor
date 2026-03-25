"""Public package API for fastapi-doctor."""

from importlib.metadata import PackageNotFoundError, version as metadata_version

from .app_loader import build_app_for_doctor
from .cli import main
from .models import DoctorIssue, DoctorReport
from .project import ProjectLayout, get_project_layout
from .runner import run_python_doctor_checks

try:
    from ._version import version as __version__
except ImportError:
    try:
        __version__ = metadata_version("fastapi-doctor")
    except PackageNotFoundError:
        __version__ = "0.0.0"

__all__ = [
    "DoctorIssue",
    "DoctorReport",
    "ProjectLayout",
    "__version__",
    "build_app_for_doctor",
    "get_project_layout",
    "main",
    "run_python_doctor_checks",
]
