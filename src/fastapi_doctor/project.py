from __future__ import annotations

"""Project discovery, runtime configuration, and file enumeration."""

import ast
import os
import tomllib
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

def _load_doctor_config() -> dict[str, Any]:
    """Load .fastapi-doctor.yml from REPO_ROOT, with a fallback to .python-doctor.yml."""
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
        import yaml  # noqa: E402 — late import; yaml is optional
        with open(config_path) as f:
            data = yaml.safe_load(f)
        return data if isinstance(data, dict) else {}
    except Exception:
        return {}

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

def _resolve_path(value: str | Path | None, *, base: Path) -> Path | None:
    if value is None:
        return None
    path = Path(value)
    if not path.is_absolute():
        path = (base / path).resolve()
    return path.resolve()

def _should_skip_path(path: Path, *, repo_root: Path) -> bool:
    try:
        rel_parts = path.relative_to(repo_root).parts
    except ValueError:
        return True
    return any(part in _EXCLUDED_DISCOVERY_DIRS or part.startswith(".") for part in rel_parts)

def _iter_repo_python_files(repo_root: Path) -> list[Path]:
    """Iterate Python files in the repo, using os.scandir for speed."""
    repo_root_str = str(repo_root)
    results: list[Path] = []
    
    def _walk(current_path: str):
        try:
            with os.scandir(current_path) as entries:
                for entry in entries:
                    if entry.is_dir(follow_symlinks=False):
                        name = entry.name
                        if name in _EXCLUDED_DISCOVERY_DIRS or name.startswith("."):
                            continue
                        _walk(entry.path)
                    elif entry.is_file(follow_symlinks=False) and entry.name.endswith(".py"):
                        results.append(Path(entry.path))
        except (PermissionError, OSError):
            pass

    _walk(repo_root_str)
    return sorted(results)

def _looks_like_fastapi_call(node: ast.AST | None) -> bool:
    if not isinstance(node, ast.Call):
        return False
    func = node.func
    return (isinstance(func, ast.Name) and func.id == "FastAPI") or (
        isinstance(func, ast.Attribute) and func.attr == "FastAPI"
    )

def _looks_like_fastapi_annotation(node: ast.AST | None) -> bool:
    return (isinstance(node, ast.Name) and node.id == "FastAPI") or (
        isinstance(node, ast.Attribute) and node.attr == "FastAPI"
    )

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
        # Quick text pre-filter: skip files that can't define a FastAPI app
        if "FastAPI" not in source and not any(name in source for name in _APP_FACTORY_NAMES):
            continue
        try:
            tree = ast.parse(source)
        except Exception:
            continue

        for stmt in tree.body:
            attr_name: str | None = None
            reason: str | None = None
            if isinstance(stmt, ast.Assign) and _looks_like_fastapi_call(stmt.value):
                names = [target.id for target in stmt.targets if isinstance(target, ast.Name)]
                if names:
                    attr_name = "app" if "app" in names else names[0]
                    reason = "module-level FastAPI app"
            elif isinstance(stmt, ast.AnnAssign) and isinstance(stmt.target, ast.Name) and _looks_like_fastapi_call(stmt.value):
                attr_name = stmt.target.id
                reason = "annotated FastAPI app"
            elif isinstance(stmt, ast.FunctionDef) and stmt.name in _APP_FACTORY_NAMES:
                has_fastapi_return = _looks_like_fastapi_annotation(stmt.returns) or any(
                    _looks_like_fastapi_call(child.value)
                    for child in ast.walk(stmt)
                    if isinstance(child, ast.Return)
                )
                if has_fastapi_return:
                    attr_name = f"{stmt.name}()"
                    reason = "FastAPI factory"

            if attr_name is None or reason is None:
                continue

            score = _score_app_candidate(file_path, attr_name)
            if best is None or score > best[0]:
                best = (score, file_path, attr_name, reason)

    if best is None:
        return None
    _, file_path, attr_name, reason = best
    return file_path, attr_name, reason

def _discover_code_dir(repo_root: Path) -> Path:
    packaged_code_dir = _discover_code_dir_from_pyproject(repo_root)
    if packaged_code_dir is not None:
        return packaged_code_dir

    candidates: list[tuple[int, Path]] = []
    for child in repo_root.iterdir():
        if not child.is_dir() or child.name in _EXCLUDED_DISCOVERY_DIRS or child.name.startswith("."):
            continue
        score = 0
        if (child / "__init__.py").exists():
            score += 10
        if (child / "routers").is_dir() or (child / "api").is_dir():
            score += 30
        if (child / "main.py").is_file() or (child / "app.py").is_file():
            score += 25
        py_count = _count_py_files(child)
        score += min(py_count, 20)
        if score:
            candidates.append((score, child))

    if candidates:
        return max(candidates, key=lambda item: item[0])[1]
    return repo_root


def _discover_code_dir_from_pyproject(repo_root: Path) -> Path | None:
    pyproject_path = repo_root / "pyproject.toml"
    if not pyproject_path.is_file():
        return None
    try:
        pyproject = tomllib.loads(pyproject_path.read_text())
    except Exception:
        return None

    search_roots = (repo_root, repo_root / "src", repo_root / "backend")

    def resolve_package_dir(package_name: str) -> Path | None:
        package_parts = tuple(part for part in package_name.replace("-", "_").split("/") if part)
        if not package_parts:
            return None
        for search_root in search_roots:
            candidate = search_root.joinpath(*package_parts)
            if candidate.is_dir():
                return candidate
        return None

    wheel_cfg = (
        pyproject.get("tool", {})
        .get("hatch", {})
        .get("build", {})
        .get("targets", {})
        .get("wheel", {})
    )
    for package_name in wheel_cfg.get("packages", []):
        if isinstance(package_name, str) and (candidate := resolve_package_dir(package_name)) is not None:
            return candidate

    project_name = pyproject.get("project", {}).get("name")
    if isinstance(project_name, str):
        return resolve_package_dir(project_name)
    return None


def _count_py_files(directory: Path, *, cap: int = 20) -> int:
    """Fast .py file count using os.scandir, stopping early at *cap*."""
    count = 0

    def _walk(current: str) -> None:
        nonlocal count
        if count >= cap:
            return
        try:
            with os.scandir(current) as it:
                for entry in it:
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
            pass

    _walk(str(directory))
    return count


def _infer_layout_from_app_module(repo_root: Path, app_module: str) -> tuple[Path, Path] | None:
    module_path, _, _ = app_module.partition(":")
    if not module_path:
        return None

    module_parts = module_path.split(".")
    if not module_parts:
        return None

    search_roots = (
        repo_root / "src",
        repo_root / "backend",
        repo_root,
    )
    for import_root in search_roots:
        package_dir = import_root.joinpath(*module_parts[:-1]) if len(module_parts) > 1 else import_root
        module_file = import_root.joinpath(*module_parts).with_suffix(".py")
        package_init = import_root.joinpath(*module_parts, "__init__.py")
        if module_file.is_file():
            code_dir = import_root / module_parts[0]
            return import_root, code_dir if code_dir.exists() else package_dir
        if package_init.is_file():
            code_dir = import_root / module_parts[0]
            return import_root, code_dir if code_dir.exists() else import_root.joinpath(*module_parts)
    return None

def _discover_project_layout(*, static_only: bool = False) -> ProjectLayout:
    repo_root = _resolve_path(os.environ.get("DOCTOR_REPO_ROOT"), base=Path.cwd()) or Path.cwd().resolve()
    explicit_code_dir = _resolve_path(os.environ.get("DOCTOR_CODE_DIR"), base=repo_root)
    explicit_import_root = _resolve_path(os.environ.get("DOCTOR_IMPORT_ROOT"), base=repo_root)
    explicit_app_module = os.environ.get("DOCTOR_APP_MODULE")

    discovery_source = "explicit overrides"
    discovered_candidate = None
    inferred_layout = None
    if explicit_app_module and (not explicit_code_dir or not explicit_import_root):
        inferred_layout = _infer_layout_from_app_module(repo_root, explicit_app_module)
        if inferred_layout is not None:
            candidate_import_root, candidate_code_dir = inferred_layout
            candidate_module = explicit_app_module.split(":", 1)[0]
            discovery_source = "explicit app module"
    if explicit_code_dir and not explicit_import_root and inferred_layout is None:
        candidate_code_dir = explicit_code_dir
        candidate_import_root = explicit_code_dir.parent
        candidate_module = explicit_code_dir.name if (explicit_code_dir / "__init__.py").is_file() else None
        candidate_attr = "app"
        discovery_source = "explicit code dir"
    elif (
        not static_only
        and inferred_layout is None
        and (not explicit_app_module or not explicit_code_dir or not explicit_import_root)
    ):
        discovered_candidate = _discover_app_candidate(repo_root)

    if discovered_candidate is not None:
        candidate_file, candidate_attr, candidate_reason = discovered_candidate
        candidate_import_root, candidate_code_dir, candidate_module = _module_context_from_file(candidate_file, repo_root)
        discovery_source = f"auto ({candidate_reason})"
    elif inferred_layout is None:
        candidate_import_root = repo_root
        candidate_code_dir = _discover_code_dir(repo_root)
        entrypoint_file = next(
            (
                candidate_code_dir / name
                for name in ("main.py", "app.py", "api.py", "server.py")
                if (candidate_code_dir / name).is_file()
            ),
            None,
        )
        if candidate_code_dir == repo_root or entrypoint_file is None:
            candidate_module = None
        else:
            import_root, _, module = _module_context_from_file(entrypoint_file, repo_root)
            candidate_import_root = import_root
            candidate_module = module
        candidate_attr = "app"
        if not explicit_app_module:
            discovery_source = (
                "static-only heuristics" if static_only else "auto (package heuristics)"
            )

    import_root = explicit_import_root or candidate_import_root
    code_dir = explicit_code_dir or candidate_code_dir
    app_module = explicit_app_module or (f"{candidate_module}:{candidate_attr}" if candidate_module else None)

    return ProjectLayout(
        repo_root=repo_root,
        import_root=import_root,
        code_dir=code_dir,
        app_module=app_module,
        discovery_source=discovery_source,
    )

def refresh_runtime_config(*, static_only: bool = False) -> ProjectLayout:
    global REPO_ROOT, IMPORT_ROOT, OWN_CODE_DIR, APP_MODULE, VENDORED_LIB
    global _PROJECT_LAYOUT
    global _DOCTOR_CONFIG, _ARCH_CONFIG, _PYDANTIC_CONFIG, _API_CONFIG, _SECURITY_CONFIG, _SCAN_CONFIG
    global ARCHITECTURE_ENABLED, GIANT_FUNCTION_THRESHOLD, LARGE_FUNCTION_THRESHOLD
    global GOD_MODULE_THRESHOLD, DEEP_NESTING_THRESHOLD, _IMPORT_BLOAT_THRESHOLD
    global _FAT_ROUTE_HANDLER_THRESHOLD, SHOULD_BE_MODEL_MODE
    global FORBIDDEN_WRITE_PARAMS, POST_CREATE_PREFIXES, TAG_REQUIRED_PREFIXES, SCAN_EXCLUDED_DIRS
    global EXCLUDE_RULES

    global _CONFIG_SIGNATURE, _PARSED_MODULE_CACHE, _LIBRARY_INFO_CACHE, _STATIC_ONLY_DISCOVERY

    layout = _discover_project_layout(static_only=static_only)
    REPO_ROOT = layout.repo_root
    IMPORT_ROOT = layout.import_root
    OWN_CODE_DIR = layout.code_dir
    APP_MODULE = layout.app_module
    VENDORED_LIB = None
    _PROJECT_LAYOUT = layout

    _DOCTOR_CONFIG = _load_doctor_config()
    _ARCH_CONFIG = _DOCTOR_CONFIG.get("architecture", {})
    _PYDANTIC_CONFIG = _DOCTOR_CONFIG.get("pydantic", {})
    _API_CONFIG = _DOCTOR_CONFIG.get("api", {})
    _SECURITY_CONFIG = _DOCTOR_CONFIG.get("security", {})
    _SCAN_CONFIG = _DOCTOR_CONFIG.get("scan", {})

    ARCHITECTURE_ENABLED = _ARCH_CONFIG.get("enabled", True)
    GIANT_FUNCTION_THRESHOLD = _ARCH_CONFIG.get("giant_function", 400)
    LARGE_FUNCTION_THRESHOLD = _ARCH_CONFIG.get("large_function", 200)
    GOD_MODULE_THRESHOLD = _ARCH_CONFIG.get("god_module", 1500)
    DEEP_NESTING_THRESHOLD = _ARCH_CONFIG.get("deep_nesting", 5)
    _IMPORT_BLOAT_THRESHOLD = _ARCH_CONFIG.get("import_bloat", 30)
    _FAT_ROUTE_HANDLER_THRESHOLD = _ARCH_CONFIG.get("fat_route_handler", 100)
    SHOULD_BE_MODEL_MODE = _PYDANTIC_CONFIG.get("should_be_model", "boundary")
    FORBIDDEN_WRITE_PARAMS = frozenset(_SECURITY_CONFIG.get("forbidden_write_params", []))
    POST_CREATE_PREFIXES = tuple(_API_CONFIG.get("create_post_prefixes", []))
    TAG_REQUIRED_PREFIXES = tuple(_API_CONFIG.get("tag_required_prefixes", ["/api/"]))
    SCAN_EXCLUDED_DIRS = frozenset(_SCAN_CONFIG.get("exclude_dirs", ["lib", "vendor", "vendored", "third_party"]))
    EXCLUDE_RULES = frozenset(_SCAN_CONFIG.get("exclude_rules", []))
    _STATIC_ONLY_DISCOVERY = static_only
    _CONFIG_SIGNATURE = _current_config_signature(static_only=static_only)
    _PARSED_MODULE_CACHE = None
    _LIBRARY_INFO_CACHE = None

    return layout

def _current_config_signature(*, static_only: bool | None = None) -> tuple[str | None, str | None, str | None, str | None, str]:
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


@dataclass(slots=True)
class LibraryInfo:
    """Detected library stack for the project."""
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

def discover_libraries() -> LibraryInfo:
    """Detect libraries from the target project, not the doctor's own environment."""
    global _LIBRARY_INFO_CACHE
    ensure_runtime_config()
    if _LIBRARY_INFO_CACHE is not None:
        return _LIBRARY_INFO_CACHE
    info = LibraryInfo()
    search_paths = [
        REPO_ROOT / "pyproject.toml",
        REPO_ROOT / "backend" / "pyproject.toml",
        REPO_ROOT / "requirements.txt",
        REPO_ROOT / "backend" / "requirements.txt",
        REPO_ROOT / "uv.lock",
        REPO_ROOT / "poetry.lock",
    ]
    
    dep_text = ""
    for p in search_paths:
        if p.exists():
            try:
                dep_text += p.read_text() + "\n"
            except Exception:
                continue

    keywords = {
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

    dep_text = dep_text.lower()
    for attr, kw in keywords.items():
        if kw in dep_text:
            setattr(info, attr, True)

    if any(getattr(info, attr) for attr in keywords):
        _LIBRARY_INFO_CACHE = info
        return info

    import_markers = {attr: False for attr in keywords}
    for module in parsed_python_modules():
        for node in ast.walk(module.tree):
            if isinstance(node, ast.Import):
                for alias in node.names:
                    base_name = alias.name.split(".", 1)[0]
                    for attr, kw in keywords.items():
                        if base_name == kw:
                            import_markers[attr] = True
            elif isinstance(node, ast.ImportFrom) and node.module:
                base_name = node.module.split(".", 1)[0]
                for attr, kw in keywords.items():
                    if base_name == kw:
                        import_markers[attr] = True

    for attr, present in import_markers.items():
        if present:
            setattr(info, attr, True)

    _LIBRARY_INFO_CACHE = info
    return info

def own_python_files() -> list[Path]:
    """All Python files under the discovered code directory."""
    ensure_runtime_config()
    if not OWN_CODE_DIR.exists():
        return []
    results: list[Path] = []

    def _walk(current_path: str, *, top_level: str | None = None) -> None:
        try:
            with os.scandir(current_path) as entries:
                for entry in entries:
                    name = entry.name
                    if name == "__pycache__" or name.startswith("."):
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
    "discover_libraries",
    "ensure_runtime_config",
    "get_effective_config",
    "get_project_layout",
    "own_python_files",
    "parsed_python_modules",
    "refresh_runtime_config",
]
