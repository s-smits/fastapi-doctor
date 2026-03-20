from __future__ import annotations

"""Configuration hygiene checks."""

import ast
import re
from dataclasses import dataclass
from pathlib import Path

from .. import project
from ..models import DoctorIssue


_NOISY_SCAN_DIRS = frozenset(
    {
        "__pycache__",
        "node_modules",
        "site-packages",
        "dist",
        "build",
        ".venv",
        "venv",
    }
)


def _iter_alembic_env_files() -> list[Path]:
    env_files: list[Path] = []
    for filepath in project.REPO_ROOT.rglob("env.py"):
        try:
            rel_parts = filepath.relative_to(project.REPO_ROOT).parts
        except ValueError:
            continue
        if "alembic" not in rel_parts and "migrations" not in rel_parts:
            continue
        if any(part.startswith(".") or part in _NOISY_SCAN_DIRS for part in rel_parts):
            continue
        env_files.append(filepath)
    return sorted(env_files)


def _parse_tree(filepath: Path) -> tuple[str, ast.AST] | None:
    try:
        source = filepath.read_text()
        return source, ast.parse(source)
    except Exception:
        return None


@dataclass(slots=True)
class _AlembicEnv:
    filepath: Path
    tree: ast.AST
    bindings: dict[str, ast.AST]
    configure_calls: list[ast.Call]


_ALEMBIC_ENV_CACHE: tuple[Path, list[_AlembicEnv]] | None = None


def _get_alembic_envs() -> list[_AlembicEnv]:
    """Cached alembic env.py discovery, parsing, and analysis."""
    global _ALEMBIC_ENV_CACHE
    if _ALEMBIC_ENV_CACHE is not None and _ALEMBIC_ENV_CACHE[0] == project.REPO_ROOT:
        return _ALEMBIC_ENV_CACHE[1]
    result: list[_AlembicEnv] = []
    for filepath in _iter_alembic_env_files():
        parsed = _parse_tree(filepath)
        if parsed is None:
            continue
        _, tree = parsed
        bindings = _module_level_bindings(tree)
        configure_calls = _find_configure_calls(tree)
        result.append(_AlembicEnv(
            filepath=filepath,
            tree=tree,
            bindings=bindings,
            configure_calls=configure_calls,
        ))
    _ALEMBIC_ENV_CACHE = (project.REPO_ROOT, result)
    return result


def _is_configure_call(node: ast.AST) -> bool:
    return (
        isinstance(node, ast.Call)
        and isinstance(node.func, ast.Attribute)
        and node.func.attr == "configure"
    )


def _find_configure_calls(tree: ast.AST) -> list[ast.Call]:
    return [node for node in ast.walk(tree) if _is_configure_call(node)]


def _module_level_bindings(tree: ast.AST) -> dict[str, ast.AST]:
    bindings: dict[str, ast.AST] = {}
    if not isinstance(tree, ast.Module):
        return bindings
    for statement in tree.body:
        if isinstance(statement, ast.Assign):
            for target in statement.targets:
                if isinstance(target, ast.Name):
                    bindings[target.id] = statement.value
        elif isinstance(statement, ast.AnnAssign) and isinstance(statement.target, ast.Name):
            value = statement.value or ast.Constant(None)
            bindings[statement.target.id] = value
    return bindings


def _value_is_non_none_metadata(value: ast.AST, bindings: dict[str, ast.AST]) -> bool:
    if isinstance(value, ast.Constant) and value.value is None:
        return False
    if isinstance(value, ast.Name) and value.id in bindings:
        bound_value = bindings[value.id]
        return not (isinstance(bound_value, ast.Constant) and bound_value.value is None)
    return True


def _has_non_none_target_metadata(call: ast.Call, bindings: dict[str, ast.AST]) -> bool:
    for keyword in call.keywords:
        if keyword.arg != "target_metadata":
            continue
        return _value_is_non_none_metadata(keyword.value, bindings)
    return False


def _has_keyword(call: ast.Call, keyword_name: str) -> bool:
    return any(keyword.arg == keyword_name for keyword in call.keywords)


def _configure_call_line(call: ast.Call) -> int:
    return getattr(call, "lineno", 1)


def _has_naming_convention() -> bool:
    for module in project.parsed_python_modules():
        if "naming_convention" not in module.source:
            continue
        for node in ast.walk(module.tree):
            if isinstance(node, ast.Call):
                func = node.func
                is_metadata_call = (
                    isinstance(func, ast.Name) and func.id == "MetaData"
                ) or (
                    isinstance(func, ast.Attribute) and func.attr == "MetaData"
                )
                if is_metadata_call and any(keyword.arg == "naming_convention" for keyword in node.keywords):
                    return True
            elif isinstance(node, ast.Assign):
                if any(isinstance(target, ast.Attribute) and target.attr == "naming_convention" for target in node.targets):
                    return True
            elif isinstance(node, ast.AnnAssign):
                if isinstance(node.target, ast.Attribute) and node.target.attr == "naming_convention":
                    return True
    return False


def check_direct_env_access() -> list[DoctorIssue]:
    """Production code should centralize env reads behind a config/settings layer."""
    issues: list[DoctorIssue] = []
    # Only check router/service code, not config/startup/scripts
    check_dirs = {"routers", "services", "interfaces"}
    # Patterns that are OK (setting defaults, not reading)
    ok_patterns = re.compile(r"os\.environ\.setdefault|os\.environ\[.+\]\s*=|os\.environ\.get\(.+,")

    for module in project.parsed_python_modules():
        parts = module.path.relative_to(project.OWN_CODE_DIR).parts
        if not parts or parts[0] not in check_dirs:
            continue
        lines = module.source.splitlines()
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
                            path=module.rel_path,
                            category="Config",
                            help="Read env vars in one config/settings module, then inject the typed setting where needed.",
                            line=i,
                        )
                    )
    return issues


def check_alembic_target_metadata() -> list[DoctorIssue]:
    """Alembic autogenerate should be wired to a real SQLAlchemy metadata object."""
    if not project.discover_libraries().alembic:
        return []

    issues: list[DoctorIssue] = []
    for env in _get_alembic_envs():
        if env.configure_calls and any(_has_non_none_target_metadata(call, env.bindings) for call in env.configure_calls):
            continue
        line = _configure_call_line(env.configure_calls[0]) if env.configure_calls else 1
        issues.append(
            DoctorIssue(
                check="config/alembic-target-metadata",
                severity="warning",
                message="Alembic env.py does not pass a real target_metadata object for autogenerate",
                path=str(env.filepath.relative_to(project.REPO_ROOT)),
                category="Config",
                help="Import your SQLAlchemy or SQLModel metadata and pass it to context.configure(target_metadata=...).",
                line=line,
            )
        )
    return issues




def check_alembic_empty_autogen_revision() -> list[DoctorIssue]:
    """Autogenerate should skip creating empty revision files."""
    if not project.discover_libraries().alembic:
        return []

    issues: list[DoctorIssue] = []
    for env in _get_alembic_envs():
        active_calls = [call for call in env.configure_calls if _has_non_none_target_metadata(call, env.bindings)]
        if not active_calls:
            continue
        if any(_has_keyword(call, "process_revision_directives") for call in active_calls):
            continue
        issues.append(
            DoctorIssue(
                check="config/alembic-empty-autogen-revision",
                severity="warning",
                message="Alembic autogenerate will still create empty revisions",
                path=str(env.filepath.relative_to(project.REPO_ROOT)),
                category="Config",
                help="Wire process_revision_directives in env.py and drop empty autogenerated revisions before the file is written.",
                line=_configure_call_line(active_calls[0]),
            )
        )
    return issues


def check_sqlalchemy_naming_convention() -> list[DoctorIssue]:
    """Alembic-managed metadata should use SQLAlchemy naming conventions."""
    libraries = project.discover_libraries()
    if not libraries.alembic or not (libraries.sqlalchemy or libraries.sqlmodel):
        return []
    if _has_naming_convention():
        return []

    for env in _get_alembic_envs():
        active_calls = [call for call in env.configure_calls if _has_non_none_target_metadata(call, env.bindings)]
        if not active_calls:
            continue
        return [
            DoctorIssue(
                check="config/sqlalchemy-naming-convention",
                severity="warning",
                message="Alembic-managed metadata has no SQLAlchemy naming_convention",
                path=str(env.filepath.relative_to(project.REPO_ROOT)),
                category="Config",
                help="Set MetaData(naming_convention=...) on your DeclarativeBase or SQLModel metadata so constraint names stay deterministic.",
                line=_configure_call_line(active_calls[0]),
            )
        ]
    return []


__all__ = [
    "check_alembic_empty_autogen_revision",
    "check_alembic_target_metadata",
    "check_direct_env_access",
    "check_sqlalchemy_naming_convention",
]
