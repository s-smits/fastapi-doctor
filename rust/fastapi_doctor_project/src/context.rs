use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

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
pub struct ArchitectureSettings {
    pub enabled: bool,
    pub giant_function: usize,
    pub large_function: usize,
    pub god_module: usize,
    pub deep_nesting: usize,
    pub import_bloat: usize,
    pub fat_route_handler: usize,
}

impl Default for ArchitectureSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            giant_function: 400,
            large_function: 200,
            god_module: 1500,
            deep_nesting: 5,
            import_bloat: 30,
            fat_route_handler: 100,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PydanticSettings {
    pub should_be_model: String,
}

impl Default for PydanticSettings {
    fn default() -> Self {
        Self {
            should_be_model: "boundary".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApiSettings {
    pub create_post_prefixes: Vec<String>,
    pub tag_required_prefixes: Vec<String>,
}

impl Default for ApiSettings {
    fn default() -> Self {
        Self {
            create_post_prefixes: Vec::new(),
            tag_required_prefixes: vec!["/api/".to_string()],
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SecuritySettings {
    pub forbidden_write_params: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ScanSettings {
    pub exclude_dirs: Vec<String>,
    pub exclude_rules: Vec<String>,
}

impl Default for ScanSettings {
    fn default() -> Self {
        Self {
            exclude_dirs: vec![
                "lib".to_string(),
                "vendor".to_string(),
                "vendored".to_string(),
                "third_party".to_string(),
            ],
            exclude_rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EffectiveProjectConfig {
    pub config_path: Option<PathBuf>,
    pub uses_legacy_config_name: bool,
    pub architecture: ArchitectureSettings,
    pub pydantic: PydanticSettings,
    pub api: ApiSettings,
    pub security: SecuritySettings,
    pub scan: ScanSettings,
}

#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub layout: ProjectLayout,
    pub libraries: LibraryInfo,
    pub effective_config: EffectiveProjectConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DoctorConfigFile {
    architecture: ArchitectureConfigFile,
    pydantic: PydanticConfigFile,
    api: ApiConfigFile,
    security: SecurityConfigFile,
    scan: ScanConfigFile,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ArchitectureConfigFile {
    enabled: Option<bool>,
    giant_function: Option<usize>,
    large_function: Option<usize>,
    god_module: Option<usize>,
    deep_nesting: Option<usize>,
    import_bloat: Option<usize>,
    fat_route_handler: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PydanticConfigFile {
    should_be_model: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ApiConfigFile {
    create_post_prefixes: Option<Vec<String>>,
    tag_required_prefixes: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SecurityConfigFile {
    forbidden_write_params: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ScanConfigFile {
    exclude_dirs: Option<Vec<String>>,
    exclude_rules: Option<Vec<String>>,
}

const EXCLUDED_DISCOVERY_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".venv",
    "venv",
    "__pycache__",
    "node_modules",
    "dist",
    "build",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    "docs",
    "frontend",
    "tests",
    "test",
    "scripts",
    "migrations",
    "alembic",
    "tmp",
    "vendor",
    "third_party",
    "lib",
    "site-packages",
    "egg-info",
    "dist-info",
    "__pypackages__",
];

const APP_FACTORY_NAMES: &[&str] = &["create_app", "build_app", "make_app", "get_app"];

pub fn resolve_project_context(static_only: bool) -> ProjectContext {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let repo_root = env_path("DOCTOR_REPO_ROOT", &cwd).unwrap_or_else(|| cwd.clone());
    let effective_config = load_effective_project_config(&repo_root);
    let explicit_code_dir = env_path("DOCTOR_CODE_DIR", &repo_root);
    let explicit_import_root = env_path("DOCTOR_IMPORT_ROOT", &repo_root);
    let explicit_app_module = env::var("DOCTOR_APP_MODULE").ok();

    let mut import_root = explicit_import_root
        .clone()
        .unwrap_or_else(|| repo_root.clone());
    let mut code_dir = explicit_code_dir
        .clone()
        .unwrap_or_else(|| repo_root.clone());
    let mut candidate_module = None;
    let mut candidate_attr = "app".to_string();
    let mut discovery_source = "explicit overrides".to_string();

    if let Some(app_module) = explicit_app_module.as_deref() {
        if (explicit_code_dir.is_none() || explicit_import_root.is_none())
            && infer_layout_from_app_module(&repo_root, app_module).is_some()
        {
            let (candidate_import_root, candidate_code_dir) =
                infer_layout_from_app_module(&repo_root, app_module).unwrap();
            import_root = explicit_import_root
                .clone()
                .unwrap_or(candidate_import_root);
            code_dir = explicit_code_dir.clone().unwrap_or(candidate_code_dir);
            discovery_source = "explicit app module".to_string();
        }
    } else if explicit_code_dir.is_some() && explicit_import_root.is_none() {
        import_root = code_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| repo_root.clone());
        if code_dir.join("__init__.py").is_file() {
            candidate_module = code_dir
                .file_name()
                .map(|name| name.to_string_lossy().to_string());
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
        app_module: explicit_app_module.or_else(|| {
            candidate_module.map(|module_name| format!("{module_name}:{candidate_attr}"))
        }),
        discovery_source,
    };
    let libraries = discover_libraries(&layout);
    ProjectContext {
        layout,
        libraries,
        effective_config,
    }
}

fn load_effective_project_config(repo_root: &Path) -> EffectiveProjectConfig {
    let mut config = EffectiveProjectConfig::default();
    let config_path = [".fastapi-doctor.yml", ".python-doctor.yml"]
        .iter()
        .map(|candidate| repo_root.join(candidate))
        .find(|path| path.is_file());

    let Some(config_path) = config_path else {
        return config;
    };

    config.uses_legacy_config_name =
        config_path.file_name().and_then(|name| name.to_str()) == Some(".python-doctor.yml");
    config.config_path = Some(config_path.clone());

    let Ok(source) = fs::read_to_string(&config_path) else {
        return config;
    };
    let parsed = serde_yaml::from_str::<DoctorConfigFile>(&source).unwrap_or_default();

    if let Some(enabled) = parsed.architecture.enabled {
        config.architecture.enabled = enabled;
    }
    if let Some(value) = parsed.architecture.giant_function {
        config.architecture.giant_function = value;
    }
    if let Some(value) = parsed.architecture.large_function {
        config.architecture.large_function = value;
    }
    if let Some(value) = parsed.architecture.god_module {
        config.architecture.god_module = value;
    }
    if let Some(value) = parsed.architecture.deep_nesting {
        config.architecture.deep_nesting = value;
    }
    if let Some(value) = parsed.architecture.import_bloat {
        config.architecture.import_bloat = value;
    }
    if let Some(value) = parsed.architecture.fat_route_handler {
        config.architecture.fat_route_handler = value;
    }

    if let Some(value) = parsed.pydantic.should_be_model {
        config.pydantic.should_be_model = value.trim().to_string();
    }

    if let Some(values) = parsed.api.create_post_prefixes {
        config.api.create_post_prefixes = sanitize_string_list(values);
    }
    if let Some(values) = parsed.api.tag_required_prefixes {
        let values = sanitize_string_list(values);
        if !values.is_empty() {
            config.api.tag_required_prefixes = values;
        }
    }

    if let Some(values) = parsed.security.forbidden_write_params {
        config.security.forbidden_write_params = sanitize_string_list(values);
    }

    if let Some(values) = parsed.scan.exclude_dirs {
        let values = sanitize_string_list(values);
        if !values.is_empty() {
            config.scan.exclude_dirs = values;
        }
    }
    if let Some(values) = parsed.scan.exclude_rules {
        config.scan.exclude_rules = sanitize_string_list(values);
    }

    config
}

fn sanitize_string_list(values: Vec<String>) -> Vec<String> {
    let mut cleaned = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        cleaned.push(trimmed.to_string());
    }
    cleaned
}

fn env_path(name: &str, base: &Path) -> Option<PathBuf> {
    env::var(name).ok().map(|value| resolve_path(&value, base))
}

fn resolve_path(value: &str, base: &Path) -> PathBuf {
    let path = PathBuf::from(value);
    let joined = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
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
            package_root
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| repo_root.to_path_buf()),
            package_root,
        ),
        None => {
            let parent = file_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| repo_root.to_path_buf());
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
    for import_root in [
        repo_root.join("src"),
        repo_root.join("backend"),
        repo_root.to_path_buf(),
    ] {
        let module_file = module_parts
            .iter()
            .fold(import_root.clone(), |path, part| path.join(part))
            .with_extension("py");
        let package_init = module_parts
            .iter()
            .fold(import_root.clone(), |path, part| path.join(part))
            .join("__init__.py");
        if module_file.is_file() || package_init.is_file() {
            let code_dir = import_root.join(module_parts[0]);
            let resolved_code_dir = if code_dir.exists() {
                code_dir
            } else if module_file.is_file() {
                module_file.parent()?.to_path_buf()
            } else {
                package_init.parent()?.to_path_buf()
            };
            return Some((import_root.clone(), resolved_code_dir));
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
        let Ok(source) = fs::read_to_string(&file_path) else {
            continue;
        };
        if !source.contains("FastAPI")
            && !APP_FACTORY_NAMES.iter().any(|name| source.contains(name))
        {
            continue;
        }
        let mut candidate = None;
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.contains("FastAPI(")
                && trimmed.contains('=')
                && !trimmed.starts_with("return ")
            {
                let Some(lhs) = trimmed.split('=').next() else {
                    continue;
                };
                let Some(lhs) = lhs.trim().split(':').next() else {
                    continue;
                };
                let lhs = lhs.trim();
                if !lhs.is_empty() {
                    let reason = if trimmed.contains(':') {
                        "annotated FastAPI app"
                    } else {
                        "module-level FastAPI app"
                    };
                    candidate = Some((lhs.to_string(), reason.to_string()));
                    break;
                }
            }
        }
        if candidate.is_none() {
            for factory_name in APP_FACTORY_NAMES {
                let marker = format!("def {factory_name}(");
                if source.contains(&marker)
                    && (source.contains("-> FastAPI") || source.contains("return FastAPI("))
                {
                    candidate = Some((format!("{factory_name}()"), "FastAPI factory".to_string()));
                    break;
                }
            }
        }
        let Some((attr_name, reason)) = candidate else {
            continue;
        };
        let score = score_app_candidate(&file_path, &attr_name);
        match best.as_ref() {
            Some((best_score, _, _, _)) if *best_score >= score => {}
            _ => {
                best = Some((score, file_path, attr_name, reason));
            }
        }
    }
    best.map(|(_, file_path, attr_name, reason)| (file_path, attr_name, reason))
}

fn score_app_candidate(file_path: &Path, attr_name: &str) -> usize {
    let mut score = 100;
    if attr_name == "app" {
        score += 40;
    }
    if attr_name.ends_with("()") {
        score += 10;
    }
    score += match file_path.file_name().and_then(|name| name.to_str()) {
        Some("main.py") => 40,
        Some("app.py") => 35,
        Some("api.py") => 25,
        Some("server.py") => 20,
        _ => 0,
    };
    for component in file_path.components() {
        if matches!(
            component.as_os_str().to_str(),
            Some("api") | Some("routers")
        ) {
            score += 10;
            break;
        }
    }
    score
}

fn discover_code_dir(repo_root: &Path) -> PathBuf {
    let Ok(children) = fs::read_dir(repo_root) else {
        return repo_root.to_path_buf();
    };
    let mut candidates = Vec::new();
    for child in children.flatten() {
        let path = child.path();
        let Ok(file_type) = child.file_type() else {
            continue;
        };
        let name = child.file_name().to_string_lossy().to_string();
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
        score += count_py_files(&path, 20);
        if score > 0 {
            candidates.push((score, path));
        }
    }
    candidates
        .into_iter()
        .max_by_key(|(score, _)| *score)
        .map(|(_, path)| path)
        .unwrap_or_else(|| repo_root.to_path_buf())
}

fn count_py_files(directory: &Path, cap: usize) -> usize {
    fn walk(current: &Path, cap: usize, count: &mut usize) {
        if *count >= cap {
            return;
        }
        let Ok(entries) = fs::read_dir(current) else {
            return;
        };
        for entry in entries.flatten() {
            if *count >= cap {
                return;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                if should_skip_name(&name) {
                    continue;
                }
                walk(&path, cap, count);
            } else if file_type.is_file() && name.ends_with(".py") {
                *count += 1;
            }
        }
    }

    let mut count = 0;
    walk(directory, cap, &mut count);
    count
}

fn discover_libraries(layout: &ProjectLayout) -> LibraryInfo {
    let mut info = LibraryInfo::default();
    let mut dep_text = String::new();
    for path in [
        layout.repo_root.join("pyproject.toml"),
        layout.repo_root.join("backend").join("pyproject.toml"),
        layout.repo_root.join("requirements.txt"),
        layout.repo_root.join("backend").join("requirements.txt"),
        layout.repo_root.join("uv.lock"),
        layout.repo_root.join("poetry.lock"),
    ] {
        if !path.exists() {
            continue;
        }
        if let Ok(content) = fs::read_to_string(path) {
            dep_text.push_str(&content);
            dep_text.push('\n');
        }
    }

    let lower_dep_text = dep_text.to_lowercase();
    info.fastapi = lower_dep_text.contains("fastapi");
    info.pydantic = lower_dep_text.contains("pydantic");
    info.sqlalchemy = lower_dep_text.contains("sqlalchemy");
    info.sqlmodel = lower_dep_text.contains("sqlmodel");
    info.django = lower_dep_text.contains("django");
    info.flask = lower_dep_text.contains("flask");
    info.httpx = lower_dep_text.contains("httpx");
    info.requests = lower_dep_text.contains("requests");
    info.alembic = lower_dep_text.contains("alembic");
    info.pytest = lower_dep_text.contains("pytest");
    info.ruff = lower_dep_text.contains("ruff");
    info.mypy = lower_dep_text.contains("mypy");
    info
}
