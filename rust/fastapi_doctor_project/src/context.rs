use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use fastapi_doctor_core::Config;
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

impl EffectiveProjectConfig {
    pub fn to_core_config(&self) -> Config {
        Config {
            import_bloat_threshold: self.architecture.import_bloat,
            giant_function_threshold: self.architecture.giant_function,
            large_function_threshold: self.architecture.large_function,
            deep_nesting_threshold: self.architecture.deep_nesting,
            god_module_threshold: self.architecture.god_module,
            fat_route_handler_threshold: self.architecture.fat_route_handler,
            should_be_model_mode: self.pydantic.should_be_model.clone(),
            forbidden_write_params: self.security.forbidden_write_params.clone(),
            create_post_prefixes: self.api.create_post_prefixes.clone(),
            tag_required_prefixes: self.api.tag_required_prefixes.clone(),
        }
    }
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
const PREFERRED_DISCOVERY_CONTAINERS: &[&str] = &["apps", "services", "packages"];

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
            let code_dir = if import_root == repo_root
                && module_parts.len() >= 2
                && PREFERRED_DISCOVERY_CONTAINERS.contains(&module_parts[0])
            {
                let nested = import_root.join(module_parts[0]).join(module_parts[1]);
                if nested.exists() {
                    nested
                } else {
                    import_root.join(module_parts[0])
                }
            } else {
                import_root.join(module_parts[0])
            };
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
    let mut candidates = Vec::new();
    for path in iter_code_dir_candidates(repo_root) {
        let score = score_code_dir_candidate(&path);
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

fn score_code_dir_candidate(path: &Path) -> usize {
    let mut score = 0;
    if path.join("__init__.py").is_file() {
        score += 10;
    }
    if path.join("routers").is_dir() || path.join("api").is_dir() {
        score += 30;
    }
    if path.join("main.py").is_file()
        || path.join("app.py").is_file()
        || path.join("api.py").is_file()
        || path.join("server.py").is_file()
    {
        score += 25;
    }
    if path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .is_some_and(|name| PREFERRED_DISCOVERY_CONTAINERS.contains(&name))
    {
        score += 20;
    }
    if matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("src") | Some("backend")
    ) {
        score += 20;
    }
    score + shallow_python_density_score(path, 12)
}

fn iter_code_dir_candidates(repo_root: &Path) -> Vec<PathBuf> {
    let Ok(children) = fs::read_dir(repo_root) else {
        return Vec::new();
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
        candidates.push(path.clone());
        if !PREFERRED_DISCOVERY_CONTAINERS.contains(&name.as_str()) {
            continue;
        }
        let Ok(nested_children) = fs::read_dir(&path) else {
            continue;
        };
        for nested_child in nested_children.flatten() {
            let nested_path = nested_child.path();
            let Ok(nested_type) = nested_child.file_type() else {
                continue;
            };
            let nested_name = nested_child.file_name().to_string_lossy().to_string();
            if !nested_type.is_dir() || should_skip_name(&nested_name) {
                continue;
            }
            candidates.push(nested_path);
        }
    }
    candidates
}

fn shallow_python_density_score(directory: &Path, cap: usize) -> usize {
    let Ok(entries) = fs::read_dir(directory) else {
        return 0;
    };

    let mut score = 0;
    for entry in entries.flatten() {
        if score >= cap {
            break;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_file() {
            if name.ends_with(".py") {
                score += 1;
            }
            continue;
        }
        if !file_type.is_dir() || should_skip_name(&name) {
            continue;
        }
        if path.join("__init__.py").is_file() {
            score += 4;
        }
        if path.join("routers").is_dir() || path.join("api").is_dir() {
            score += 3;
        }
        if path.join("main.py").is_file()
            || path.join("app.py").is_file()
            || path.join("api.py").is_file()
            || path.join("server.py").is_file()
        {
            score += 3;
        }
    }

    score.min(cap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper: create a temp directory with the given YAML config file content.
    fn tmp_project(yaml: &str) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join(".fastapi-doctor.yml");
        fs::write(&config_path, yaml).unwrap();
        tmp
    }

    // ── Config YAML parsing ──────────────────────────────────────────────

    #[test]
    fn test_default_config_when_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        let config = load_effective_project_config(tmp.path());

        assert!(config.config_path.is_none());
        assert!(!config.uses_legacy_config_name);

        // Architecture defaults
        assert!(config.architecture.enabled);
        assert_eq!(config.architecture.giant_function, 400);
        assert_eq!(config.architecture.large_function, 200);
        assert_eq!(config.architecture.god_module, 1500);
        assert_eq!(config.architecture.deep_nesting, 5);
        assert_eq!(config.architecture.import_bloat, 30);
        assert_eq!(config.architecture.fat_route_handler, 100);

        // Pydantic defaults
        assert_eq!(config.pydantic.should_be_model, "boundary");

        // API defaults
        assert!(config.api.create_post_prefixes.is_empty());
        assert_eq!(config.api.tag_required_prefixes, vec!["/api/"]);

        // Security defaults
        assert!(config.security.forbidden_write_params.is_empty());

        // Scan defaults
        assert_eq!(
            config.scan.exclude_dirs,
            vec!["lib", "vendor", "vendored", "third_party"]
        );
        assert!(config.scan.exclude_rules.is_empty());
    }

    #[test]
    fn test_full_custom_config() {
        let yaml = r#"
architecture:
  enabled: false
  giant_function: 500
  large_function: 250
  god_module: 2000
  deep_nesting: 8
  import_bloat: 50
  fat_route_handler: 120
pydantic:
  should_be_model: strict
api:
  create_post_prefixes:
    - /v1/
    - /v2/
  tag_required_prefixes:
    - /public/
security:
  forbidden_write_params:
    - password
    - secret
scan:
  exclude_dirs:
    - generated
    - proto
  exclude_rules:
    - R001
    - R002
"#;
        let tmp = tmp_project(yaml);
        let config = load_effective_project_config(tmp.path());

        assert!(config.config_path.is_some());
        assert!(!config.uses_legacy_config_name);

        assert!(!config.architecture.enabled);
        assert_eq!(config.architecture.giant_function, 500);
        assert_eq!(config.architecture.large_function, 250);
        assert_eq!(config.architecture.god_module, 2000);
        assert_eq!(config.architecture.deep_nesting, 8);
        assert_eq!(config.architecture.import_bloat, 50);
        assert_eq!(config.architecture.fat_route_handler, 120);

        assert_eq!(config.pydantic.should_be_model, "strict");

        assert_eq!(config.api.create_post_prefixes, vec!["/v1/", "/v2/"]);
        assert_eq!(config.api.tag_required_prefixes, vec!["/public/"]);

        assert_eq!(
            config.security.forbidden_write_params,
            vec!["password", "secret"]
        );

        assert_eq!(config.scan.exclude_dirs, vec!["generated", "proto"]);
        assert_eq!(config.scan.exclude_rules, vec!["R001", "R002"]);
    }

    #[test]
    fn test_partial_config_preserves_defaults() {
        let yaml = r#"
architecture:
  giant_function: 600
scan:
  exclude_rules:
    - R100
"#;
        let tmp = tmp_project(yaml);
        let config = load_effective_project_config(tmp.path());

        // Overridden field
        assert_eq!(config.architecture.giant_function, 600);
        // Other architecture fields keep defaults
        assert!(config.architecture.enabled);
        assert_eq!(config.architecture.large_function, 200);
        assert_eq!(config.architecture.god_module, 1500);
        assert_eq!(config.architecture.deep_nesting, 5);
        assert_eq!(config.architecture.import_bloat, 30);
        assert_eq!(config.architecture.fat_route_handler, 100);

        // Scan: exclude_dirs keeps default, exclude_rules is overridden
        assert_eq!(
            config.scan.exclude_dirs,
            vec!["lib", "vendor", "vendored", "third_party"]
        );
        assert_eq!(config.scan.exclude_rules, vec!["R100"]);

        // API keeps defaults
        assert_eq!(config.api.tag_required_prefixes, vec!["/api/"]);
    }

    #[test]
    fn test_empty_yaml_gives_defaults() {
        let tmp = tmp_project("");
        let config = load_effective_project_config(tmp.path());

        assert!(config.config_path.is_some());
        assert!(config.architecture.enabled);
        assert_eq!(config.architecture.giant_function, 400);
        assert_eq!(config.pydantic.should_be_model, "boundary");
    }

    #[test]
    fn test_empty_exclude_dirs_keeps_default() {
        // An empty list should be treated as "not specified" since the code
        // checks `!values.is_empty()` before overwriting.
        let yaml = r#"
scan:
  exclude_dirs: []
"#;
        let tmp = tmp_project(yaml);
        let config = load_effective_project_config(tmp.path());
        assert_eq!(
            config.scan.exclude_dirs,
            vec!["lib", "vendor", "vendored", "third_party"]
        );
    }

    #[test]
    fn test_empty_tag_required_prefixes_keeps_default() {
        let yaml = r#"
api:
  tag_required_prefixes: []
"#;
        let tmp = tmp_project(yaml);
        let config = load_effective_project_config(tmp.path());
        assert_eq!(config.api.tag_required_prefixes, vec!["/api/"]);
    }

    #[test]
    fn test_whitespace_trimming_in_string_lists() {
        let yaml = r#"
scan:
  exclude_dirs:
    - "  mydir  "
    - ""
    - "  "
    - valid
"#;
        let tmp = tmp_project(yaml);
        let config = load_effective_project_config(tmp.path());
        assert_eq!(config.scan.exclude_dirs, vec!["mydir", "valid"]);
    }

    #[test]
    fn test_should_be_model_whitespace_trimmed() {
        let yaml = r#"
pydantic:
  should_be_model: "  strict  "
"#;
        let tmp = tmp_project(yaml);
        let config = load_effective_project_config(tmp.path());
        assert_eq!(config.pydantic.should_be_model, "strict");
    }

    #[test]
    fn test_legacy_config_name_detection() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join(".python-doctor.yml");
        fs::write(&config_path, "architecture:\n  giant_function: 999\n").unwrap();

        let config = load_effective_project_config(tmp.path());
        assert!(config.uses_legacy_config_name);
        assert_eq!(config.architecture.giant_function, 999);
    }

    #[test]
    fn test_fastapi_doctor_yml_preferred_over_legacy() {
        let tmp = tempfile::tempdir().unwrap();
        // Write both config files; the new name should be preferred.
        fs::write(
            tmp.path().join(".fastapi-doctor.yml"),
            "architecture:\n  giant_function: 111\n",
        )
        .unwrap();
        fs::write(
            tmp.path().join(".python-doctor.yml"),
            "architecture:\n  giant_function: 222\n",
        )
        .unwrap();

        let config = load_effective_project_config(tmp.path());
        assert!(!config.uses_legacy_config_name);
        assert_eq!(config.architecture.giant_function, 111);
    }

    #[test]
    fn test_invalid_yaml_returns_defaults() {
        let yaml = "{{{{invalid yaml!!!!";
        let tmp = tmp_project(yaml);
        let config = load_effective_project_config(tmp.path());

        // serde_yaml::from_str returns an error, so unwrap_or_default kicks in.
        assert!(config.architecture.enabled);
        assert_eq!(config.architecture.giant_function, 400);
    }

    // ── to_core_config ───────────────────────────────────────────────────

    #[test]
    fn test_to_core_config_maps_all_fields() {
        let yaml = r#"
architecture:
  import_bloat: 42
  giant_function: 500
  large_function: 250
  deep_nesting: 7
  god_module: 2000
  fat_route_handler: 120
pydantic:
  should_be_model: strict
security:
  forbidden_write_params:
    - secret
api:
  create_post_prefixes:
    - /v1/
  tag_required_prefixes:
    - /public/
"#;
        let tmp = tmp_project(yaml);
        let effective = load_effective_project_config(tmp.path());
        let core = effective.to_core_config();

        assert_eq!(core.import_bloat_threshold, 42);
        assert_eq!(core.giant_function_threshold, 500);
        assert_eq!(core.large_function_threshold, 250);
        assert_eq!(core.deep_nesting_threshold, 7);
        assert_eq!(core.god_module_threshold, 2000);
        assert_eq!(core.fat_route_handler_threshold, 120);
        assert_eq!(core.should_be_model_mode, "strict");
        assert_eq!(core.forbidden_write_params, vec!["secret"]);
        assert_eq!(core.create_post_prefixes, vec!["/v1/"]);
        assert_eq!(core.tag_required_prefixes, vec!["/public/"]);
    }

    // ── sanitize_string_list ─────────────────────────────────────────────

    #[test]
    fn test_sanitize_string_list_basic() {
        let input = vec![
            " hello ".to_string(),
            "".to_string(),
            "  ".to_string(),
            "world".to_string(),
        ];
        let result = sanitize_string_list(input);
        assert_eq!(result, vec!["hello", "world"]);
    }

    #[test]
    fn test_sanitize_string_list_empty_input() {
        let result = sanitize_string_list(Vec::new());
        assert!(result.is_empty());
    }

    // ── should_skip_name ─────────────────────────────────────────────────

    #[test]
    fn test_should_skip_dotfiles() {
        assert!(should_skip_name(".git"));
        assert!(should_skip_name(".hidden"));
        assert!(should_skip_name("."));
    }

    #[test]
    fn test_should_skip_excluded_dirs() {
        assert!(should_skip_name("__pycache__"));
        assert!(should_skip_name("node_modules"));
        assert!(should_skip_name(".venv"));
        assert!(should_skip_name("venv"));
        assert!(should_skip_name("dist"));
        assert!(should_skip_name("build"));
        assert!(should_skip_name("tests"));
        assert!(should_skip_name("migrations"));
    }

    #[test]
    fn test_should_not_skip_regular_dirs() {
        assert!(!should_skip_name("src"));
        assert!(!should_skip_name("myapp"));
        assert!(!should_skip_name("routers"));
        assert!(!should_skip_name("api"));
    }

    // ── resolve_path ─────────────────────────────────────────────────────

    #[test]
    fn test_resolve_path_absolute() {
        let base = PathBuf::from("/some/base");
        let result = resolve_path("/absolute/path", &base);
        // The absolute path is returned as-is (not joined with base).
        // Since /absolute/path likely doesn't exist, it's returned un-canonicalized.
        assert_eq!(result, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn test_resolve_path_relative() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("sub");
        fs::create_dir(&sub).unwrap();
        // Relative path is joined with base
        let result = resolve_path("sub", tmp.path());
        // Since the path exists, it should be canonicalized
        assert!(result.ends_with("sub"));
        assert!(result.is_absolute());
    }

    // ── score_app_candidate ──────────────────────────────────────────────

    #[test]
    fn test_score_app_candidate_main_py_with_app() {
        let score = score_app_candidate(Path::new("/project/main.py"), "app");
        // base 100 + "app" 40 + "main.py" 40 = 180
        assert_eq!(score, 180);
    }

    #[test]
    fn test_score_app_candidate_app_py_with_factory() {
        let score = score_app_candidate(Path::new("/project/app.py"), "create_app()");
        // base 100 + factory "()" 10 + "app.py" 35 = 145
        assert_eq!(score, 145);
    }

    #[test]
    fn test_score_app_candidate_in_api_directory() {
        let score = score_app_candidate(Path::new("/project/api/server.py"), "app");
        // base 100 + "app" 40 + "server.py" 20 + api dir 10 = 170
        assert_eq!(score, 170);
    }

    #[test]
    fn test_score_app_candidate_unknown_filename() {
        let score = score_app_candidate(Path::new("/project/something.py"), "my_app");
        // base 100 + nothing extra = 100
        assert_eq!(score, 100);
    }

    // ── module_context_from_file ─────────────────────────────────────────

    #[test]
    fn test_module_context_flat_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let file = repo_root.join("main.py");
        fs::write(&file, "").unwrap();

        let (import_root, code_dir, module_name) = module_context_from_file(&file, repo_root);
        assert_eq!(import_root, repo_root);
        assert_eq!(code_dir, repo_root);
        assert_eq!(module_name, "main");
    }

    #[test]
    fn test_module_context_in_package() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let package = repo_root.join("myapp");
        fs::create_dir(&package).unwrap();
        fs::write(package.join("__init__.py"), "").unwrap();
        let file = package.join("main.py");
        fs::write(&file, "").unwrap();

        let (import_root, code_dir, module_name) = module_context_from_file(&file, repo_root);
        assert_eq!(import_root, repo_root.to_path_buf());
        assert_eq!(code_dir, package);
        assert_eq!(module_name, "myapp.main");
    }

    #[test]
    fn test_module_context_nested_package() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let pkg = repo_root.join("myapp");
        let sub = pkg.join("api");
        fs::create_dir_all(&sub).unwrap();
        fs::write(pkg.join("__init__.py"), "").unwrap();
        fs::write(sub.join("__init__.py"), "").unwrap();
        let file = sub.join("routes.py");
        fs::write(&file, "").unwrap();

        let (import_root, code_dir, module_name) = module_context_from_file(&file, repo_root);
        assert_eq!(import_root, repo_root.to_path_buf());
        assert_eq!(code_dir, pkg);
        assert_eq!(module_name, "myapp.api.routes");
    }

    // ── discover_code_dir & iter_code_dir_candidates ─────────────────────

    #[test]
    fn test_discover_code_dir_prefers_package_with_router() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let app_dir = repo.join("myapp");
        fs::create_dir_all(app_dir.join("routers")).unwrap();
        fs::write(app_dir.join("__init__.py"), "").unwrap();
        fs::write(app_dir.join("main.py"), "").unwrap();

        // Also create a less interesting directory
        let scripts = repo.join("utils");
        fs::create_dir(&scripts).unwrap();

        let result = discover_code_dir(repo);
        assert_eq!(result, app_dir);
    }

    #[test]
    fn test_discover_code_dir_falls_back_to_repo_root() {
        let tmp = tempfile::tempdir().unwrap();
        // No interesting subdirectories
        let result = discover_code_dir(tmp.path());
        assert_eq!(result, tmp.path().to_path_buf());
    }

    // ── discover_libraries ───────────────────────────────────────────────

    #[test]
    fn test_discover_libraries_from_pyproject_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let pyproject = r#"
[project]
dependencies = [
    "fastapi>=0.100",
    "pydantic>=2",
    "sqlalchemy[asyncio]",
    "httpx",
    "alembic",
]

[tool.pytest]
testpaths = ["tests"]
"#;
        fs::write(repo.join("pyproject.toml"), pyproject).unwrap();

        let layout = ProjectLayout {
            repo_root: repo.to_path_buf(),
            import_root: repo.to_path_buf(),
            code_dir: repo.to_path_buf(),
            app_module: None,
            discovery_source: "test".to_string(),
        };
        let libs = discover_libraries(&layout);

        assert!(libs.fastapi);
        assert!(libs.pydantic);
        assert!(libs.sqlalchemy);
        assert!(libs.httpx);
        assert!(libs.alembic);
        assert!(libs.pytest);
        assert!(!libs.django);
        assert!(!libs.flask);
        assert!(!libs.sqlmodel);
    }

    #[test]
    fn test_discover_libraries_from_requirements_txt() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        fs::write(repo.join("requirements.txt"), "Flask\nrequests\nmypy\n").unwrap();

        let layout = ProjectLayout {
            repo_root: repo.to_path_buf(),
            import_root: repo.to_path_buf(),
            code_dir: repo.to_path_buf(),
            app_module: None,
            discovery_source: "test".to_string(),
        };
        let libs = discover_libraries(&layout);

        assert!(libs.flask);
        assert!(libs.requests);
        assert!(libs.mypy);
        assert!(!libs.fastapi);
    }

    #[test]
    fn test_discover_libraries_no_files() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = ProjectLayout {
            repo_root: tmp.path().to_path_buf(),
            import_root: tmp.path().to_path_buf(),
            code_dir: tmp.path().to_path_buf(),
            app_module: None,
            discovery_source: "test".to_string(),
        };
        let libs = discover_libraries(&layout);

        assert!(!libs.fastapi);
        assert!(!libs.pydantic);
        assert!(!libs.sqlalchemy);
    }

    // ── iter_repo_python_files ───────────────────────────────────────────

    #[test]
    fn test_iter_repo_python_files_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("main.py"), "").unwrap();
        fs::write(root.join("readme.md"), "").unwrap();
        let sub = root.join("myapp");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("routes.py"), "").unwrap();

        let mut files = Vec::new();
        iter_repo_python_files(root, &mut files);
        let names: Vec<&str> = files
            .iter()
            .map(|f| f.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(names.contains(&"main.py"));
        assert!(names.contains(&"routes.py"));
        assert!(!names.contains(&"readme.md"));
    }

    #[test]
    fn test_iter_repo_python_files_skips_excluded() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // File in a directory that should be skipped
        let venv = root.join(".venv");
        fs::create_dir(&venv).unwrap();
        fs::write(venv.join("some.py"), "").unwrap();

        let cache = root.join("__pycache__");
        fs::create_dir(&cache).unwrap();
        fs::write(cache.join("cached.py"), "").unwrap();

        // File in a normal directory
        fs::write(root.join("good.py"), "").unwrap();

        let mut files = Vec::new();
        iter_repo_python_files(root, &mut files);
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("good.py"));
    }

    // ── infer_layout_from_app_module ─────────────────────────────────────

    #[test]
    fn test_infer_layout_simple_module() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let myapp = repo.join("myapp");
        fs::create_dir(&myapp).unwrap();
        fs::write(myapp.join("main.py"), "").unwrap();

        let result = infer_layout_from_app_module(repo, "myapp.main:app");
        assert!(result.is_some());
        let (import_root, code_dir) = result.unwrap();
        assert_eq!(import_root, repo.to_path_buf());
        assert_eq!(code_dir, myapp);
    }

    #[test]
    fn test_infer_layout_src_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let src = repo.join("src");
        let myapp = src.join("myapp");
        fs::create_dir_all(&myapp).unwrap();
        fs::write(myapp.join("main.py"), "").unwrap();

        let result = infer_layout_from_app_module(repo, "myapp.main:app");
        assert!(result.is_some());
        let (import_root, code_dir) = result.unwrap();
        assert_eq!(import_root, src);
        assert_eq!(code_dir, src.join("myapp"));
    }

    #[test]
    fn test_infer_layout_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let result = infer_layout_from_app_module(tmp.path(), "nonexistent.module:app");
        assert!(result.is_none());
    }
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
