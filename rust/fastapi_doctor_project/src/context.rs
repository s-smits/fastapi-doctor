use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProjectLayout {
    pub repo_root: PathBuf,
    pub import_root: PathBuf,
    pub code_dir: PathBuf,
    pub app_module: Option<String>,
    pub discovery_source: String,
}

#[derive(Debug, Clone, Default)]
pub struct LibraryInfo {
    pub fastapi: bool,
    pub pydantic: bool,
    pub sqlalchemy: bool,
    pub sqlmodel: bool,
    pub django: bool,
    pub flask: bool,
    pub httpx: bool,
    pub requests: bool,
    pub alembic: bool,
    pub pytest: bool,
    pub ruff: bool,
    pub mypy: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub layout: ProjectLayout,
    pub libraries: LibraryInfo,
}

const EXCLUDED_DISCOVERY_DIRS: &[&str] = &[
    ".git", ".hg", ".svn", ".venv", "venv", "__pycache__", "node_modules", "dist", "build",
    ".mypy_cache", ".pytest_cache", ".ruff_cache", "docs", "frontend", "tests", "test",
    "scripts", "migrations", "alembic", "tmp", "vendor", "third_party", "lib",
    "site-packages", "egg-info", "dist-info", "__pypackages__",
];

const APP_FACTORY_NAMES: &[&str] = &["create_app", "build_app", "make_app", "get_app"];

pub fn resolve_project_context(static_only: bool) -> ProjectContext {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let repo_root = env_path("DOCTOR_REPO_ROOT", &cwd).unwrap_or_else(|| cwd.clone());
    let explicit_code_dir = env_path("DOCTOR_CODE_DIR", &repo_root);
    let explicit_import_root = env_path("DOCTOR_IMPORT_ROOT", &repo_root);
    let explicit_app_module = env::var("DOCTOR_APP_MODULE").ok();

    let mut import_root = explicit_import_root.clone().unwrap_or_else(|| repo_root.clone());
    let mut code_dir = explicit_code_dir.clone().unwrap_or_else(|| repo_root.clone());
    let mut candidate_module = None;
    let mut candidate_attr = "app".to_string();
    let mut discovery_source = "explicit overrides".to_string();

    if let Some(app_module) = explicit_app_module.as_deref() {
        if (explicit_code_dir.is_none() || explicit_import_root.is_none())
            && infer_layout_from_app_module(&repo_root, app_module).is_some()
        {
            let (candidate_import_root, candidate_code_dir) =
                infer_layout_from_app_module(&repo_root, app_module).unwrap();
            import_root = explicit_import_root.clone().unwrap_or(candidate_import_root);
            code_dir = explicit_code_dir.clone().unwrap_or(candidate_code_dir);
            discovery_source = "explicit app module".to_string();
        }
    } else if explicit_code_dir.is_some() && explicit_import_root.is_none() {
        import_root = code_dir.parent().map(Path::to_path_buf).unwrap_or_else(|| repo_root.clone());
        if code_dir.join("__init__.py").is_file() {
            candidate_module = code_dir.file_name().map(|name| name.to_string_lossy().to_string());
        }
        discovery_source = "explicit code dir".to_string();
    } else if !static_only {
        if let Some((file_path, attr_name, reason)) = discover_app_candidate(&repo_root) {
            let (candidate_import_root, candidate_code_dir, module_name) =
                module_context_from_file(&file_path, &repo_root);
            import_root = candidate_import_root;
            code_dir = candidate_code_dir;
            candidate_module = Some(module_name);
            candidate_attr = attr_name;
            discovery_source = format!("auto ({reason})");
        }
    }

    if code_dir == repo_root && (static_only || candidate_module.is_none()) {
        code_dir = discover_code_dir(&repo_root);
        if code_dir != repo_root {
            if let Some(entrypoint_file) = ["main.py", "app.py", "api.py", "server.py"]
                .iter()
                .map(|name| code_dir.join(name))
                .find(|path| path.is_file())
            {
                let (candidate_import_root, _, module_name) =
                    module_context_from_file(&entrypoint_file, &repo_root);
                if explicit_import_root.is_none() {
                    import_root = candidate_import_root;
                }
                candidate_module = Some(module_name);
            }
        }
        if explicit_app_module.is_none() && discovery_source == "explicit overrides" {
            discovery_source = if static_only {
                "static-only heuristics".to_string()
            } else {
                "auto (package heuristics)".to_string()
            };
        }
    }

    let layout = ProjectLayout {
        repo_root: repo_root.clone(),
        import_root,
        code_dir,
        app_module: explicit_app_module
            .or_else(|| candidate_module.map(|module_name| format!("{module_name}:{candidate_attr}"))),
        discovery_source,
    };
    let libraries = discover_libraries(&layout);
    ProjectContext { layout, libraries }
}

fn env_path(name: &str, base: &Path) -> Option<PathBuf> {
    env::var(name).ok().map(|value| resolve_path(&value, base))
}

fn resolve_path(value: &str, base: &Path) -> PathBuf {
    let path = PathBuf::from(value);
    let joined = if path.is_absolute() { path } else { base.join(path) };
    if joined.exists() {
        fs::canonicalize(&joined).unwrap_or(joined)
    } else {
        joined
    }
}

fn should_skip_name(name: &str) -> bool {
    name.starts_with('.') || EXCLUDED_DISCOVERY_DIRS.contains(&name)
}

fn iter_repo_python_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if !should_skip_name(&name) {
                iter_repo_python_files(&path, out);
            }
        } else if file_type.is_file() && name.ends_with(".py") {
            out.push(path);
        }
    }
}

fn module_context_from_file(file_path: &Path, repo_root: &Path) -> (PathBuf, PathBuf, String) {
    let mut package_root = None;
    let mut current = file_path.parent().unwrap_or(repo_root).to_path_buf();
    while current != repo_root && current.join("__init__.py").is_file() {
        package_root = Some(current.clone());
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent.to_path_buf();
    }
    let (import_root, code_dir) = match package_root {
        Some(package_root) => (
            package_root.parent().map(Path::to_path_buf).unwrap_or_else(|| repo_root.to_path_buf()),
            package_root,
        ),
        None => {
            let parent = file_path.parent().map(Path::to_path_buf).unwrap_or_else(|| repo_root.to_path_buf());
            (parent.clone(), parent)
        }
    };
    let module_name = file_path
        .strip_prefix(&import_root)
        .unwrap_or(file_path)
        .with_extension("")
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(".");
    (import_root, code_dir, module_name)
}

fn infer_layout_from_app_module(repo_root: &Path, app_module: &str) -> Option<(PathBuf, PathBuf)> {
    let module_path = app_module.split(':').next()?;
    let module_parts = module_path.split('.').collect::<Vec<_>>();
    for import_root in [repo_root.join("src"), repo_root.join("backend"), repo_root.to_path_buf()] {
        let module_file = module_parts.iter().fold(import_root.clone(), |path, part| path.join(part)).with_extension("py");
        if module_file.is_file() {
            let code_dir = import_root.join(module_parts[0]);
            return Some((import_root.clone(), if code_dir.exists() { code_dir } else { module_file.parent()?.to_path_buf() }));
        }
    }
    None
}

fn discover_app_candidate(repo_root: &Path) -> Option<(PathBuf, String, String)> {
    let mut files = Vec::new();
    iter_repo_python_files(repo_root, &mut files);
    files.sort();
    let mut best = None;
    for file_path in files {
        let Ok(source) = fs::read_to_string(&file_path) else { continue };
        if !source.contains("FastAPI") && !APP_FACTORY_NAMES.iter().any(|name| source.contains(name)) {
            continue;
        }
        let mut candidate = None;
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.contains("FastAPI(") && trimmed.contains('=') && !trimmed.starts_with("return ") {
                let Some(lhs) = trimmed.split('=').next() else {
                    continue;
                };
                let Some(lhs) = lhs.trim().split(':').next() else {
                    continue;
                };
                let lhs = lhs.trim();
                if !lhs.is_empty() {
                    let reason = if trimmed.contains(':') { "annotated FastAPI app" } else { "module-level FastAPI app" };
                    candidate = Some((lhs.to_string(), reason.to_string()));
                    break;
                }
            }
        }
        if candidate.is_none() {
            for factory_name in APP_FACTORY_NAMES {
                let marker = format!("def {factory_name}(");
                if source.contains(&marker) && (source.contains("-> FastAPI") || source.contains("return FastAPI(")) {
                    candidate = Some((format!("{factory_name}()"), "FastAPI factory".to_string()));
                    break;
                }
            }
        }
        let Some((attr_name, reason)) = candidate else { continue };
        let mut score = 100;
        if attr_name == "app" {
            score += 40;
        }
        if file_path.to_string_lossy().contains("/api/") || file_path.to_string_lossy().contains("/routers/") {
            score += 10;
        }
        score += match file_path.file_name().and_then(|name| name.to_str()) {
            Some("main.py") => 40,
            Some("app.py") => 35,
            Some("api.py") => 25,
            Some("server.py") => 20,
            _ => 0,
        };
        match &best {
            Some((best_score, _, _, _)) if *best_score >= score => {}
            _ => best = Some((score, file_path, attr_name, reason)),
        }
    }
    best.map(|(_, path, attr_name, reason)| (path, attr_name, reason))
}

fn discover_code_dir(repo_root: &Path) -> PathBuf {
    if let Some(code_dir) = discover_code_dir_from_pyproject(repo_root) {
        return code_dir;
    }
    let Ok(entries) = fs::read_dir(repo_root) else {
        return repo_root.to_path_buf();
    };
    let mut best = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(file_type) = entry.file_type() else { continue };
        if !file_type.is_dir() || should_skip_name(&name) {
            continue;
        }
        let mut score = 0;
        if path.join("__init__.py").is_file() {
            score += 10;
        }
        if path.join("routers").is_dir() || path.join("api").is_dir() {
            score += 30;
        }
        if path.join("main.py").is_file() || path.join("app.py").is_file() {
            score += 25;
        }
        if score > 0 {
            best = match best {
                Some((best_score, best_path)) if best_score >= score => Some((best_score, best_path)),
                _ => Some((score, path)),
            };
        }
    }
    best.map(|(_, path)| path).unwrap_or_else(|| repo_root.to_path_buf())
}

fn discover_code_dir_from_pyproject(repo_root: &Path) -> Option<PathBuf> {
    let pyproject_path = repo_root.join("pyproject.toml");
    let text = fs::read_to_string(pyproject_path).ok()?;
    let mut in_project = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_project = trimmed == "[project]";
            continue;
        }
        if in_project && trimmed.starts_with("name") {
            let Some((_, raw)) = trimmed.split_once('=') else {
                continue;
            };
            let project_name = raw.trim().trim_matches('"').trim_matches('\'').replace('-', "_");
            for search_root in [repo_root.to_path_buf(), repo_root.join("src"), repo_root.join("backend")] {
                let candidate = search_root.join(&project_name);
                if candidate.is_dir() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn discover_libraries(layout: &ProjectLayout) -> LibraryInfo {
    let mut info = LibraryInfo::default();
    let mut dependency_text = String::new();
    for path in [
        layout.repo_root.join("pyproject.toml"),
        layout.repo_root.join("backend").join("pyproject.toml"),
        layout.repo_root.join("requirements.txt"),
        layout.repo_root.join("backend").join("requirements.txt"),
        layout.repo_root.join("uv.lock"),
        layout.repo_root.join("poetry.lock"),
    ] {
        if let Ok(text) = fs::read_to_string(path) {
            dependency_text.push_str(&text.to_lowercase());
        }
    }
    mark_library_keywords(&dependency_text, &mut info);
    if has_any_library(&info) {
        return info;
    }

    let mut files = Vec::new();
    iter_repo_python_files(&layout.code_dir, &mut files);
    for file_path in files {
        if let Ok(source) = fs::read_to_string(file_path) {
            mark_library_keywords(&source.to_lowercase(), &mut info);
        }
    }
    info
}

fn mark_library_keywords(text: &str, info: &mut LibraryInfo) {
    info.fastapi |= text.contains("fastapi");
    info.pydantic |= text.contains("pydantic");
    info.sqlalchemy |= text.contains("sqlalchemy");
    info.sqlmodel |= text.contains("sqlmodel");
    info.django |= text.contains("django");
    info.flask |= text.contains("flask");
    info.httpx |= text.contains("httpx");
    info.requests |= text.contains("requests");
    info.alembic |= text.contains("alembic");
    info.pytest |= text.contains("pytest");
    info.ruff |= text.contains("ruff");
    info.mypy |= text.contains("mypy");
}

fn has_any_library(info: &LibraryInfo) -> bool {
    info.fastapi
        || info.pydantic
        || info.sqlalchemy
        || info.sqlmodel
        || info.django
        || info.flask
        || info.httpx
        || info.requests
        || info.alembic
        || info.pytest
        || info.ruff
        || info.mypy
}
