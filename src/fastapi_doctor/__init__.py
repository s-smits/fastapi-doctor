"""Public package API for fastapi-doctor."""

from importlib import import_module

from ._compat import get_installed_version

__version__ = get_installed_version()

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


def __getattr__(name: str):
    if name in {"DoctorIssue", "DoctorReport"}:
        module = import_module(".models", __name__)
        return getattr(module, name)
    if name in {"ProjectLayout", "get_project_layout"}:
        module = import_module(".project", __name__)
        return getattr(module, name)
    if name == "build_app_for_doctor":
        module = import_module(".app_loader", __name__)
        return module.build_app_for_doctor
    if name == "run_python_doctor_checks":
        module = import_module(".runner", __name__)
        return module.run_python_doctor_checks
    if name == "main":
        module = import_module(".cli", __name__)
        return module.main
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
