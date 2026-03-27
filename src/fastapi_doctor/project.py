from __future__ import annotations

"""Rust-first project context adapter plus Python compatibility helpers."""

import ast
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Any


PROTECTED_ROUTE_RULES: tuple[tuple[str, tuple[frozenset[str], ...]], ...] = ()
FORBIDDEN_WRITE_PARAMS: frozenset[str] = frozenset()
POST_CREATE_PREFIXES: tuple[str, ...] = ()
TAG_REQUIRED_PREFIXES: tuple[str, ...] = ("/api/",)
EXCLUDE_RULES: frozenset[str] = frozenset()

REPO_ROOT = Path.cwd().resolve()
IMPORT_ROOT = REPO_ROOT
OWN_CODE_DIR = REPO_ROOT
APP_MODULE: str | None = None
VENDORED_LIB: Path | None = None
SCAN_EXCLUDED_DIRS: frozenset[str] = frozenset({"lib", "vendor", "vendored", "third_party"})


@dataclass(slots=True)
class ProjectLayout:
    repo_root: Path
    import_root: Path
    code_dir: Path
    app_module: str | None
    discovery_source: str


@dataclass(slots=True)
class ParsedModule:
    path: Path
    rel_path: str
    source: str
    tree: ast.AST


@dataclass(slots=True)
class LibraryInfo:
    fastapi: bool = False
    pydantic: bool = False
    sqlalchemy: bool = False
    sqlmodel: bool = False
    django: bool = False
    flask: bool = False
    httpx: bool = False
    requests: bool = False
    alembic: bool = False
    pytest: bool = False
    ruff: bool = False
    mypy: bool = False


_PROJECT_LAYOUT = ProjectLayout(
    repo_root=REPO_ROOT,
    import_root=IMPORT_ROOT,
    code_dir=OWN_CODE_DIR,
    app_module=APP_MODULE,
    discovery_source="uninitialized",
)
_STATIC_ONLY_DISCOVERY = False
_CONFIG_SIGNATURE: tuple[str | None, str | None, str | None, str | None, str, str] | None = None
_PARSED_MODULE_CACHE: tuple[
    tuple[str | None, str | None, str | None, str | None, str, str],
    list[ParsedModule],
] | None = None
_LIBRARY_INFO_CACHE: LibraryInfo | None = None
_NATIVE_PROJECT_CONTEXT: dict[str, Any] | None = None

_DOCTOR_CONFIG: dict[str, Any] = {}
_ARCH_CONFIG: dict[str, Any] = {}
_PYDANTIC_CONFIG: dict[str, Any] = {}
_API_CONFIG: dict[str, Any] = {}
_SECURITY_CONFIG: dict[str, Any] = {}
_SCAN_CONFIG: dict[str, Any] = {}
_CONFIG_PATH: Path | None = None

ARCHITECTURE_ENABLED: bool = True
GIANT_FUNCTION_THRESHOLD: int = 400
LARGE_FUNCTION_THRESHOLD: int = 200
GOD_MODULE_THRESHOLD: int = 1500
DEEP_NESTING_THRESHOLD: int = 5
_IMPORT_BLOAT_THRESHOLD: int = 30
_FAT_ROUTE_HANDLER_THRESHOLD: int = 100
SHOULD_BE_MODEL_MODE: str = "boundary"

ASYNC_ENDPOINT_NOAWAIT_EXCLUDE = frozenset()

DEFAULT_ENV: dict[str, str] = {
    "SIMPLE_BOOT": "1",
    "OPENAI_API_KEY": "fastapi-doctor-openai-key",
    "OS_SECURITY_KEY": "fastapi-doctor-os-security-key-32-chars-minimum",
}

_EXCLUDED_DISCOVERY_DIRS = frozenset(
    {
        ".git", ".hg", ".svn", ".venv", "venv", "__pycache__",
        "node_modules", "dist", "build", ".mypy_cache", ".pytest_cache",
        ".ruff_cache", "docs", "frontend", "tests", "test", "scripts",
        "migrations", "alembic", "tmp", "vendor", "third_party", "lib",
        "site-packages", "egg-info", "dist-info", "__pypackages__",
    }
)
_APP_FILE_BONUS = {"main.py": 40, "app.py": 35, "api.py": 25, "server.py": 20}
_APP_FACTORY_NAMES = frozenset({"create_app", "build_app", "make_app", "get_app"})
_LIBRARY_KEYWORDS = {
    "fastapi": "fastapi",
    "pydantic": "pydantic",
    "sqlalchemy": "sqlalchemy",
    "sqlmodel": "sqlmodel",
    "django": "django",
    "flask": "flask",
    "httpx": "httpx",
    "requests": "requests",
    "alembic": "alembic",
    "pytest": "pytest",
    "ruff": "ruff",
    "mypy": "mypy",
}


def _resolve_path(value: str | Path | None, *, base: Path) -> Path | None:
    if value is None:
        return None
    path = Path(value)
    if not path.is_absolute():
        path = (base / path).resolve()
    return path.resolve()


def _load_native_project_context(*, static_only: bool) -> dict[str, Any] | None:
    try:
        from . import _fastapi_doctor_native
    except Exception:
        return None
    try:
        payload = _fastapi_doctor_native.get_project_context(static_only=static_only)
    except Exception:
        return None
    return payload if isinstance(payload, dict) else None


def _library_info_from_payload(payload: dict[str, Any]) -> LibraryInfo:
    return LibraryInfo(
        fastapi=bool(payload.get("fastapi")),
        pydantic=bool(payload.get("pydantic")),
        sqlalchemy=bool(payload.get("sqlalchemy")),
        sqlmodel=bool(payload.get("sqlmodel")),
        django=bool(payload.get("django")),
        flask=bool(payload.get("flask")),
        httpx=bool(payload.get("httpx")),
        requests=bool(payload.get("requests")),
        alembic=bool(payload.get("alembic")),
        pytest=bool(payload.get("pytest")),
        ruff=bool(payload.get("ruff")),
        mypy=bool(payload.get("mypy")),
    )


def _native_effective_config(native_context: dict[str, Any] | None) -> dict[str, Any] | None:
    if not native_context:
        return None
    payload = native_context.get("effective_config")
    return payload if isinstance(payload, dict) else None


def apply_native_project_context(native_context: dict[str, Any], *, static_only: bool = False) -> ProjectLayout:
    global REPO_ROOT, IMPORT_ROOT, OWN_CODE_DIR, APP_MODULE, VENDORED_LIB
    global _PROJECT_LAYOUT
    global _CONFIG_SIGNATURE, _PARSED_MODULE_CACHE, _LIBRARY_INFO_CACHE, _STATIC_ONLY_DISCOVERY
    global _NATIVE_PROJECT_CONTEXT

    native_layout = native_context["layout"]
    layout = ProjectLayout(
        repo_root=Path(native_layout["repo_root"]),
        import_root=Path(native_layout["import_root"]),
        code_dir=Path(native_layout["code_dir"]),
        app_module=native_layout.get("app_module"),
        discovery_source=native_layout["discovery_source"],
    )

    REPO_ROOT = layout.repo_root
    IMPORT_ROOT = layout.import_root
    OWN_CODE_DIR = layout.code_dir
    APP_MODULE = layout.app_module
    VENDORED_LIB = None
    _PROJECT_LAYOUT = layout
    _NATIVE_PROJECT_CONTEXT = native_context

    native_effective_config = _native_effective_config(native_context)
    if native_effective_config is not None:
        _apply_effective_config(native_effective_config)
    else:
        _apply_effective_config(_load_doctor_config())

    _STATIC_ONLY_DISCOVERY = static_only
    _CONFIG_SIGNATURE = _current_config_signature(static_only=static_only)
    _PARSED_MODULE_CACHE = None
    _LIBRARY_INFO_CACHE = None

    if isinstance(native_context.get("libraries"), dict):
        _LIBRARY_INFO_CACHE = _library_info_from_payload(native_context["libraries"])

    return layout


def _as_string_list(value: Any) -> list[str]:
    if not isinstance(value, list):
        return []
    return [item.strip() for item in value if isinstance(item, str) and item.strip()]


def _as_int(value: Any, default: int) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def _apply_effective_config(effective_config: dict[str, Any]) -> None:
    global _CONFIG_PATH
    global _DOCTOR_CONFIG, _ARCH_CONFIG, _PYDANTIC_CONFIG, _API_CONFIG, _SECURITY_CONFIG, _SCAN_CONFIG
    global ARCHITECTURE_ENABLED, GIANT_FUNCTION_THRESHOLD, LARGE_FUNCTION_THRESHOLD
    global GOD_MODULE_THRESHOLD, DEEP_NESTING_THRESHOLD, _IMPORT_BLOAT_THRESHOLD
    global _FAT_ROUTE_HANDLER_THRESHOLD, SHOULD_BE_MODEL_MODE
    global FORBIDDEN_WRITE_PARAMS, POST_CREATE_PREFIXES, TAG_REQUIRED_PREFIXES, SCAN_EXCLUDED_DIRS
    global EXCLUDE_RULES

    _DOCTOR_CONFIG = effective_config
    config_path = effective_config.get("config_path")
    _CONFIG_PATH = Path(config_path) if isinstance(config_path, str) and config_path else None

    _ARCH_CONFIG = effective_config.get("architecture", {}) if isinstance(effective_config.get("architecture"), dict) else {}
    _PYDANTIC_CONFIG = effective_config.get("pydantic", {}) if isinstance(effective_config.get("pydantic"), dict) else {}
    _API_CONFIG = effective_config.get("api", {}) if isinstance(effective_config.get("api"), dict) else {}
    _SECURITY_CONFIG = effective_config.get("security", {}) if isinstance(effective_config.get("security"), dict) else {}
    _SCAN_CONFIG = effective_config.get("scan", {}) if isinstance(effective_config.get("scan"), dict) else {}

    ARCHITECTURE_ENABLED = bool(_ARCH_CONFIG.get("enabled", True))
    GIANT_FUNCTION_THRESHOLD = _as_int(_ARCH_CONFIG.get("giant_function"), 400)
    LARGE_FUNCTION_THRESHOLD = _as_int(_ARCH_CONFIG.get("large_function"), 200)
    GOD_MODULE_THRESHOLD = _as_int(_ARCH_CONFIG.get("god_module"), 1500)
    DEEP_NESTING_THRESHOLD = _as_int(_ARCH_CONFIG.get("deep_nesting"), 5)
    _IMPORT_BLOAT_THRESHOLD = _as_int(_ARCH_CONFIG.get("import_bloat"), 30)
    _FAT_ROUTE_HANDLER_THRESHOLD = _as_int(_ARCH_CONFIG.get("fat_route_handler"), 100)
    SHOULD_BE_MODEL_MODE = str(_PYDANTIC_CONFIG.get("should_be_model") or "boundary")
    FORBIDDEN_WRITE_PARAMS = frozenset(_as_string_list(_SECURITY_CONFIG.get("forbidden_write_params")))
    POST_CREATE_PREFIXES = tuple(_as_string_list(_API_CONFIG.get("create_post_prefixes")))
    tag_required_prefixes = _as_string_list(_API_CONFIG.get("tag_required_prefixes"))
    TAG_REQUIRED_PREFIXES = tuple(tag_required_prefixes or ["/api/"])
    scan_excluded_dirs = _as_string_list(_SCAN_CONFIG.get("exclude_dirs"))
    SCAN_EXCLUDED_DIRS = frozenset(scan_excluded_dirs or ["lib", "vendor", "vendored", "third_party"])
    EXCLUDE_RULES = frozenset(_as_string_list(_SCAN_CONFIG.get("exclude_rules")))


def _load_doctor_config() -> dict[str, Any]:
    global _CONFIG_PATH
    config_path = next(
        (
            REPO_ROOT / candidate
            for candidate in (".fastapi-doctor.yml", ".python-doctor.yml")
            if (REPO_ROOT / candidate).is_file()
        ),
        None,
    )
    _CONFIG_PATH = config_path
    if config_path is None:
        return {}
    try:
        import yaml  # noqa: E402

        with config_path.open() as f:
            data = yaml.safe_load(f)
        loaded = data if isinstance(data, dict) else {}
        return {
            "config_path": str(config_path),
            "uses_legacy_config_name": config_path.name == ".python-doctor.yml",
            "architecture": loaded.get("architecture", {}) if isinstance(loaded.get("architecture"), dict) else {},
            "pydantic": loaded.get("pydantic", {}) if isinstance(loaded.get("pydantic"), dict) else {},
            "api": loaded.get("api", {}) if isinstance(loaded.get("api"), dict) else {},
            "security": loaded.get("security", {}) if isinstance(loaded.get("security"), dict) else {},
            "scan": loaded.get("scan", {}) if isinstance(loaded.get("scan"), dict) else {},
        }
    except Exception:
        return {}


def _iter_repo_python_files(repo_root: Path) -> list[Path]:
    results: list[Path] = []

    def _walk(current_path: str) -> None:
        try:
            with os.scandir(current_path) as entries:
                for entry in entries:
                    name = entry.name
                    if name.startswith(".") or name in _EXCLUDED_DISCOVERY_DIRS:
                        continue
                    if entry.is_dir(follow_symlinks=False):
                        _walk(entry.path)
                    elif entry.is_file(follow_symlinks=False) and name.endswith(".py"):
                        results.append(Path(entry.path))
        except (PermissionError, OSError):
            return

    _walk(str(repo_root))
    return sorted(results)


def _module_context_from_file(file_path: Path, repo_root: Path) -> tuple[Path, Path, str]:
    package_root: Path | None = None
    current = file_path.parent
    while current != repo_root and (current / "__init__.py").exists():
        package_root = current
        current = current.parent

    if package_root is None:
        import_root = file_path.parent
        code_dir = file_path.parent
    else:
        import_root = package_root.parent
        code_dir = package_root

    module_parts = file_path.relative_to(import_root).with_suffix("").parts
    module_name = ".".join(module_parts)
    return import_root, code_dir, module_name


def _infer_layout_from_app_module(repo_root: Path, app_module: str) -> tuple[Path, Path] | None:
    module_path, _, _ = app_module.partition(":")
    if not module_path:
        return None

    module_parts = module_path.split(".")
    for import_root in (repo_root / "src", repo_root / "backend", repo_root):
        module_file = import_root.joinpath(*module_parts).with_suffix(".py")
        package_init = import_root.joinpath(*module_parts, "__init__.py")
        if module_file.is_file():
            code_dir = import_root / module_parts[0]
            return import_root, code_dir if code_dir.exists() else module_file.parent
        if package_init.is_file():
            code_dir = import_root / module_parts[0]
            return import_root, code_dir if code_dir.exists() else package_init.parent
    return None


def _score_app_candidate(file_path: Path, attr_name: str) -> int:
    score = 100
    if attr_name == "app":
        score += 40
    if attr_name.endswith("()"):
        score += 10
    score += _APP_FILE_BONUS.get(file_path.name, 0)
    rel_parts = file_path.parts
    if "api" in rel_parts or "routers" in rel_parts:
        score += 10
    return score


def _discover_app_candidate(repo_root: Path) -> tuple[Path, str, str] | None:
    best: tuple[int, Path, str, str] | None = None
    for file_path in _iter_repo_python_files(repo_root):
        try:
            source = file_path.read_text()
        except Exception:
            continue
        if "FastAPI" not in source and not any(name in source for name in _APP_FACTORY_NAMES):
            continue

        candidate: tuple[str, str] | None = None
        for line in source.splitlines():
            trimmed = line.strip()
            if "FastAPI(" in trimmed and "=" in trimmed and not trimmed.startswith("return "):
                lhs = trimmed.split("=", 1)[0].split(":", 1)[0].strip()
                if lhs:
                    candidate = (
                        lhs,
                        "annotated FastAPI app" if ":" in trimmed else "module-level FastAPI app",
                    )
                    break
        if candidate is None:
            for factory_name in _APP_FACTORY_NAMES:
                marker = f"def {factory_name}("
                if marker in source and ("-> FastAPI" in source or "return FastAPI(" in source):
                    candidate = (f"{factory_name}()", "FastAPI factory")
                    break

        if candidate is None:
            continue
        attr_name, reason = candidate
        score = _score_app_candidate(file_path, attr_name)
        if best is None or score > best[0]:
            best = (score, file_path, attr_name, reason)

    if best is None:
        return None
    _, file_path, attr_name, reason = best
    return file_path, attr_name, reason


def _discover_code_dir(repo_root: Path) -> Path:
    candidates: list[tuple[int, Path]] = []
    try:
        children = list(repo_root.iterdir())
    except OSError:
        return repo_root
    for child in children:
        if not child.is_dir() or child.name.startswith(".") or child.name in _EXCLUDED_DISCOVERY_DIRS:
            continue
        score = 0
        if (child / "__init__.py").exists():
            score += 10
        if (child / "routers").is_dir() or (child / "api").is_dir():
            score += 30
        if (child / "main.py").is_file() or (child / "app.py").is_file():
            score += 25
        score += min(_count_py_files(child), 20)
        if score:
            candidates.append((score, child))
    return max(candidates, key=lambda item: item[0])[1] if candidates else repo_root


def _count_py_files(directory: Path, *, cap: int = 20) -> int:
    count = 0

    def _walk(current_path: str) -> None:
        nonlocal count
        if count >= cap:
            return
        try:
            with os.scandir(current_path) as entries:
                for entry in entries:
                    if count >= cap:
                        return
                    name = entry.name
                    if name.startswith(".") or name in _EXCLUDED_DISCOVERY_DIRS:
                        continue
                    if entry.is_dir(follow_symlinks=False):
                        _walk(entry.path)
                    elif entry.is_file(follow_symlinks=False) and name.endswith(".py"):
                        count += 1
        except (PermissionError, OSError):
            return

    _walk(str(directory))
    return count


def _fallback_project_layout(*, static_only: bool = False) -> ProjectLayout:
    repo_root = _resolve_path(os.environ.get("DOCTOR_REPO_ROOT"), base=Path.cwd()) or Path.cwd().resolve()
    explicit_code_dir = _resolve_path(os.environ.get("DOCTOR_CODE_DIR"), base=repo_root)
    explicit_import_root = _resolve_path(os.environ.get("DOCTOR_IMPORT_ROOT"), base=repo_root)
    explicit_app_module = os.environ.get("DOCTOR_APP_MODULE")

    import_root = explicit_import_root or repo_root
    code_dir = explicit_code_dir or repo_root
    app_module = explicit_app_module
    discovery_source = "explicit overrides"

    if explicit_app_module and (explicit_code_dir is None or explicit_import_root is None):
        inferred = _infer_layout_from_app_module(repo_root, explicit_app_module)
        if inferred is not None:
            import_root = explicit_import_root or inferred[0]
            code_dir = explicit_code_dir or inferred[1]
            discovery_source = "explicit app module"
    elif explicit_code_dir and explicit_import_root is None:
        import_root = explicit_code_dir.parent
        discovery_source = "explicit code dir"
    elif not static_only:
        candidate = _discover_app_candidate(repo_root)
        if candidate is not None:
            candidate_file, candidate_attr, reason = candidate
            import_root, code_dir, module_name = _module_context_from_file(candidate_file, repo_root)
            app_module = f"{module_name}:{candidate_attr}"
            discovery_source = f"auto ({reason})"

    if code_dir == repo_root and app_module is None:
        code_dir = _discover_code_dir(repo_root)
        if code_dir != repo_root:
            entrypoint_file = next(
                (
                    code_dir / name
                    for name in ("main.py", "app.py", "api.py", "server.py")
                    if (code_dir / name).is_file()
                ),
                None,
            )
            if entrypoint_file is not None:
                import_root, _, module_name = _module_context_from_file(entrypoint_file, repo_root)
                app_module = f"{module_name}:app"
        if explicit_app_module is None and discovery_source == "explicit overrides":
            discovery_source = "static-only heuristics" if static_only else "auto (package heuristics)"

    return ProjectLayout(
        repo_root=repo_root,
        import_root=import_root,
        code_dir=code_dir,
        app_module=app_module,
        discovery_source=discovery_source,
    )


def refresh_runtime_config(*, static_only: bool = False) -> ProjectLayout:
    global _NATIVE_PROJECT_CONTEXT

    native_context = _load_native_project_context(static_only=static_only)
    if native_context and isinstance(native_context.get("layout"), dict):
        return apply_native_project_context(native_context, static_only=static_only)
    else:
        layout = _fallback_project_layout(static_only=static_only)
        _NATIVE_PROJECT_CONTEXT = None

    REPO_ROOT = layout.repo_root
    IMPORT_ROOT = layout.import_root
    OWN_CODE_DIR = layout.code_dir
    APP_MODULE = layout.app_module
    VENDORED_LIB = None
    _PROJECT_LAYOUT = layout

    native_effective_config = _native_effective_config(_NATIVE_PROJECT_CONTEXT)
    if native_effective_config is not None:
        _apply_effective_config(native_effective_config)
    else:
        _apply_effective_config(_load_doctor_config())
    _STATIC_ONLY_DISCOVERY = static_only
    _CONFIG_SIGNATURE = _current_config_signature(static_only=static_only)
    _PARSED_MODULE_CACHE = None
    _LIBRARY_INFO_CACHE = None

    return layout


def _current_config_signature(*, static_only: bool | None = None) -> tuple[str | None, str | None, str | None, str | None, str, str]:
    mode = _STATIC_ONLY_DISCOVERY if static_only is None else static_only
    return (
        os.environ.get("DOCTOR_REPO_ROOT"),
        os.environ.get("DOCTOR_CODE_DIR"),
        os.environ.get("DOCTOR_IMPORT_ROOT"),
        os.environ.get("DOCTOR_APP_MODULE"),
        str(Path.cwd().resolve()),
        "1" if mode else "0",
    )


def ensure_runtime_config() -> ProjectLayout:
    if _PROJECT_LAYOUT.discovery_source == "uninitialized" or _CONFIG_SIGNATURE != _current_config_signature():
        return refresh_runtime_config()
    return _PROJECT_LAYOUT


def get_project_layout() -> ProjectLayout:
    ensure_runtime_config()
    return _PROJECT_LAYOUT


def get_effective_config() -> dict[str, Any]:
    ensure_runtime_config()
    return {
        "config_path": str(_CONFIG_PATH) if _CONFIG_PATH else None,
        "uses_legacy_config_name": bool(_CONFIG_PATH and _CONFIG_PATH.name == ".python-doctor.yml"),
        "architecture": {
            "enabled": ARCHITECTURE_ENABLED,
            "giant_function": GIANT_FUNCTION_THRESHOLD,
            "large_function": LARGE_FUNCTION_THRESHOLD,
            "god_module": GOD_MODULE_THRESHOLD,
            "deep_nesting": DEEP_NESTING_THRESHOLD,
            "import_bloat": _IMPORT_BLOAT_THRESHOLD,
            "fat_route_handler": _FAT_ROUTE_HANDLER_THRESHOLD,
        },
        "pydantic": {
            "should_be_model": SHOULD_BE_MODEL_MODE,
        },
        "api": {
            "create_post_prefixes": list(POST_CREATE_PREFIXES),
            "tag_required_prefixes": list(TAG_REQUIRED_PREFIXES),
        },
        "security": {
            "forbidden_write_params": sorted(FORBIDDEN_WRITE_PARAMS),
        },
        "scan": {
            "exclude_dirs": sorted(SCAN_EXCLUDED_DIRS),
            "exclude_rules": sorted(EXCLUDE_RULES),
        },
    }


def discover_libraries() -> LibraryInfo:
    global _LIBRARY_INFO_CACHE
    ensure_runtime_config()
    if _LIBRARY_INFO_CACHE is not None:
        return _LIBRARY_INFO_CACHE

    info = LibraryInfo()
    dep_text = ""
    for path in (
        REPO_ROOT / "pyproject.toml",
        REPO_ROOT / "backend" / "pyproject.toml",
        REPO_ROOT / "requirements.txt",
        REPO_ROOT / "backend" / "requirements.txt",
        REPO_ROOT / "uv.lock",
        REPO_ROOT / "poetry.lock",
    ):
        if not path.exists():
            continue
        try:
            dep_text += path.read_text() + "\n"
        except Exception:
            continue

    lower_dep_text = dep_text.lower()
    for attr, keyword in _LIBRARY_KEYWORDS.items():
        if keyword in lower_dep_text:
            setattr(info, attr, True)

    _LIBRARY_INFO_CACHE = info
    return info


def own_python_files() -> list[Path]:
    ensure_runtime_config()
    if not OWN_CODE_DIR.exists():
        return []

    results: list[Path] = []

    def _walk(current_path: str, *, top_level: str | None = None) -> None:
        try:
            with os.scandir(current_path) as entries:
                for entry in entries:
                    name = entry.name
                    if name.startswith(".") or name == "__pycache__":
                        continue
                    if entry.is_dir(follow_symlinks=False):
                        next_top_level = top_level or name
                        if next_top_level in SCAN_EXCLUDED_DIRS:
                            continue
                        _walk(entry.path, top_level=next_top_level)
                    elif entry.is_file(follow_symlinks=False) and name.endswith(".py"):
                        results.append(Path(entry.path))
        except (PermissionError, OSError):
            return

    _walk(str(OWN_CODE_DIR))
    return sorted(results)


def parsed_python_modules() -> list[ParsedModule]:
    global _PARSED_MODULE_CACHE
    ensure_runtime_config()
    signature = _current_config_signature()
    if _PARSED_MODULE_CACHE is not None and _PARSED_MODULE_CACHE[0] == signature:
        return _PARSED_MODULE_CACHE[1]

    modules: list[ParsedModule] = []
    for path in own_python_files():
        try:
            source = path.read_text()
            tree = ast.parse(source)
        except Exception:
            continue
        modules.append(
            ParsedModule(
                path=path,
                rel_path=str(path.relative_to(REPO_ROOT)),
                source=source,
                tree=tree,
            )
        )

    _PARSED_MODULE_CACHE = (signature, modules)
    return modules


__all__ = [
    "APP_MODULE",
    "ARCHITECTURE_ENABLED",
    "ASYNC_ENDPOINT_NOAWAIT_EXCLUDE",
    "DEFAULT_ENV",
    "DEEP_NESTING_THRESHOLD",
    "FORBIDDEN_WRITE_PARAMS",
    "GIANT_FUNCTION_THRESHOLD",
    "GOD_MODULE_THRESHOLD",
    "IMPORT_ROOT",
    "LARGE_FUNCTION_THRESHOLD",
    "OWN_CODE_DIR",
    "ParsedModule",
    "PROTECTED_ROUTE_RULES",
    "ProjectLayout",
    "REPO_ROOT",
    "SHOULD_BE_MODEL_MODE",
    "VENDORED_LIB",
    "_FAT_ROUTE_HANDLER_THRESHOLD",
    "_IMPORT_BLOAT_THRESHOLD",
    "_discover_app_candidate",
    "discover_libraries",
    "ensure_runtime_config",
    "apply_native_project_context",
    "get_effective_config",
    "get_project_layout",
    "own_python_files",
    "parsed_python_modules",
    "refresh_runtime_config",
]
