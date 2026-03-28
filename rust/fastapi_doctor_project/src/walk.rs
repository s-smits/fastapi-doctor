use std::fs;
use std::path::{Path, PathBuf};

use fastapi_doctor_core::{path_to_string, ModuleRecord};
use rayon::prelude::*;

use crate::context::resolve_project_context;
use crate::metadata::ProjectMetadata;

#[derive(Debug, Clone)]
pub struct ProjectFilesFilter {
    excluded_dirs: Vec<String>,
}

impl ProjectFilesFilter {
    pub fn from_metadata(metadata: &ProjectMetadata) -> Self {
        Self {
            excluded_dirs: metadata.excluded_dirs.clone(),
        }
    }

    pub fn is_directory_included(&self, name: &str) -> bool {
        !(name.starts_with('.')
            || name == "__pycache__"
            || self.excluded_dirs.iter().any(|candidate| candidate == name))
    }

    pub fn is_file_included(&self, name: &str) -> bool {
        name.ends_with(".py")
    }
}

#[derive(Debug, Clone)]
pub struct ProjectFilesWalker {
    repo_root: PathBuf,
    roots: Vec<PathBuf>,
    filter: ProjectFilesFilter,
}

impl ProjectFilesWalker {
    pub fn new(metadata: &ProjectMetadata) -> Self {
        Self {
            repo_root: metadata.repo_root.clone(),
            roots: vec![metadata.code_root.clone()],
            filter: ProjectFilesFilter::from_metadata(metadata),
        }
    }

    pub fn collect_paths(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for root in &self.roots {
            collect_python_files(root, &self.filter, &mut files);
        }
        collect_alembic_env_files(&self.repo_root, &mut files);
        files.sort();
        files.dedup();
        files
    }
}

#[derive(Debug, Clone)]
pub struct LoadedProject {
    pub metadata: ProjectMetadata,
    pub modules: Vec<ModuleRecord>,
}

#[derive(Debug, Clone)]
pub struct LoadedProjectBundle {
    pub context: crate::ProjectContext,
    pub project: LoadedProject,
}

pub fn load_current_project_bundle(static_only: bool) -> Result<LoadedProjectBundle, String> {
    let context = resolve_project_context(static_only);
    let metadata = ProjectMetadata::from_context(&context);
    let project = load_project_modules(metadata)?;
    Ok(LoadedProjectBundle { context, project })
}

pub fn load_project_modules(metadata: ProjectMetadata) -> Result<LoadedProject, String> {
    let walker = ProjectFilesWalker::new(&metadata);
    let repo_root = metadata.repo_root.clone();
    let modules = walker
        .collect_paths()
        .into_par_iter()
        .map(|path| {
            let source = fs::read_to_string(&path).map_err(|err| err.to_string())?;
            let rel_path = path
                .strip_prefix(&repo_root)
                .map(path_to_string)
                .unwrap_or_else(|_| path_to_string(&path));
            Ok(ModuleRecord { rel_path, source })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(LoadedProject { metadata, modules })
}

/// Find alembic env.py files starting from `repo_root`.
/// Checks common locations first, then does a 2-level directory scan.
pub fn find_alembic_env_files(repo_root: &Path) -> Vec<PathBuf> {
    let migration_dir_names = ["alembic", "migrations"];
    let search_roots = [
        repo_root.to_path_buf(),
        repo_root.join("src"),
        repo_root.join("backend"),
    ];

    // Fast path: common locations
    let mut found: Vec<PathBuf> = search_roots
        .iter()
        .flat_map(|root| {
            migration_dir_names
                .iter()
                .map(move |dir| root.join(dir).join("env.py"))
        })
        .filter(|p| p.is_file())
        .collect();

    if !found.is_empty() {
        found.sort();
        found.dedup();
        return found;
    }

    // Shallow scan: look one and two levels deep for migration dirs
    let skip = [
        "__pycache__",
        "node_modules",
        "site-packages",
        "dist",
        "build",
        ".venv",
        "venv",
        ".git",
    ];
    for search_root in &search_roots {
        let Ok(entries) = fs::read_dir(search_root) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if skip.contains(&name.as_ref()) || name.starts_with('.') {
                continue;
            }
            let dir_path = entry.path();
            // Direct child: alembic/ or migrations/
            if migration_dir_names.contains(&name.as_ref()) {
                let candidate = dir_path.join("env.py");
                if candidate.is_file() {
                    found.push(candidate);
                }
                continue;
            }
            // One level deeper
            let Ok(nested) = fs::read_dir(&dir_path) else {
                continue;
            };
            for nested_entry in nested.flatten() {
                let Ok(nft) = nested_entry.file_type() else {
                    continue;
                };
                if !nft.is_dir() {
                    continue;
                }
                let nname = nested_entry.file_name();
                let nname = nname.to_string_lossy();
                if migration_dir_names.contains(&nname.as_ref()) {
                    let candidate = nested_entry.path().join("env.py");
                    if candidate.is_file() {
                        found.push(candidate);
                    }
                }
            }
        }
    }

    found.sort();
    found.dedup();
    found
}

fn collect_python_files(root: &Path, filter: &ProjectFilesFilter, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if filter.is_directory_included(name.as_ref()) {
                collect_python_files(&path, filter, out);
            }
        } else if file_type.is_file() && filter.is_file_included(name.as_ref()) {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_metadata(
        repo_root: PathBuf,
        code_root: PathBuf,
        excluded_dirs: Vec<String>,
    ) -> ProjectMetadata {
        ProjectMetadata::new(repo_root, code_root, excluded_dirs)
    }

    // ── ProjectFilesFilter ───────────────────────────────────────────────

    #[test]
    fn test_filter_includes_python_files() {
        let meta = make_metadata(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo/src"),
            vec!["vendor".to_string()],
        );
        let filter = ProjectFilesFilter::from_metadata(&meta);

        assert!(filter.is_file_included("main.py"));
        assert!(filter.is_file_included("routes.py"));
        assert!(!filter.is_file_included("readme.md"));
        assert!(!filter.is_file_included("config.yaml"));
        assert!(!filter.is_file_included("data.json"));
        assert!(!filter.is_file_included("Makefile"));
    }

    #[test]
    fn test_filter_excludes_hidden_and_pycache() {
        let meta = make_metadata(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            vec![],
        );
        let filter = ProjectFilesFilter::from_metadata(&meta);

        assert!(!filter.is_directory_included(".git"));
        assert!(!filter.is_directory_included(".hidden"));
        assert!(!filter.is_directory_included("__pycache__"));
        assert!(filter.is_directory_included("myapp"));
        assert!(filter.is_directory_included("routers"));
    }

    #[test]
    fn test_filter_excludes_custom_dirs() {
        let meta = make_metadata(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            vec!["vendor".to_string(), "generated".to_string()],
        );
        let filter = ProjectFilesFilter::from_metadata(&meta);

        assert!(!filter.is_directory_included("vendor"));
        assert!(!filter.is_directory_included("generated"));
        assert!(filter.is_directory_included("src"));
        assert!(filter.is_directory_included("api"));
    }

    #[test]
    fn test_filter_empty_exclude_dirs() {
        let meta = make_metadata(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            vec![],
        );
        let filter = ProjectFilesFilter::from_metadata(&meta);

        // Only hidden dirs and __pycache__ are excluded
        assert!(filter.is_directory_included("vendor"));
        assert!(filter.is_directory_included("lib"));
        assert!(!filter.is_directory_included(".git"));
        assert!(!filter.is_directory_included("__pycache__"));
    }

    // ── ProjectFilesWalker ───────────────────────────────────────────────

    #[test]
    fn test_walker_collects_python_files_recursively() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let src = root.join("src");
        let sub = src.join("api");
        fs::create_dir_all(&sub).unwrap();
        fs::write(src.join("main.py"), "print('hello')").unwrap();
        fs::write(sub.join("routes.py"), "# routes").unwrap();
        fs::write(src.join("readme.md"), "# readme").unwrap();

        let meta = make_metadata(root.to_path_buf(), src.clone(), vec![]);
        let walker = ProjectFilesWalker::new(&meta);
        let paths = walker.collect_paths();

        let names: Vec<String> = paths
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();
        assert!(names.iter().any(|n| n.contains("main.py")));
        assert!(names.iter().any(|n| n.contains("routes.py")));
        assert!(!names.iter().any(|n| n.contains("readme.md")));
    }

    #[test]
    fn test_walker_respects_exclude_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let src = root.join("src");
        let vendor = src.join("vendor");
        let good = src.join("app");
        fs::create_dir_all(&vendor).unwrap();
        fs::create_dir_all(&good).unwrap();
        fs::write(vendor.join("external.py"), "").unwrap();
        fs::write(good.join("main.py"), "").unwrap();

        let meta = make_metadata(
            root.to_path_buf(),
            src.clone(),
            vec!["vendor".to_string()],
        );
        let walker = ProjectFilesWalker::new(&meta);
        let paths = walker.collect_paths();

        let names: Vec<String> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"main.py".to_string()));
        assert!(!names.contains(&"external.py".to_string()));
    }

    #[test]
    fn test_walker_deduplicates_and_sorts() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("b.py"), "").unwrap();
        fs::write(root.join("a.py"), "").unwrap();

        let meta = make_metadata(root.to_path_buf(), root.to_path_buf(), vec![]);
        let walker = ProjectFilesWalker::new(&meta);
        let paths = walker.collect_paths();
        let names: Vec<String> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        // Should be sorted
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn test_walker_empty_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = make_metadata(tmp.path().to_path_buf(), tmp.path().to_path_buf(), vec![]);
        let walker = ProjectFilesWalker::new(&meta);
        let paths = walker.collect_paths();
        assert!(paths.is_empty());
    }

    #[test]
    fn test_walker_skips_hidden_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let hidden = root.join(".hidden");
        fs::create_dir(&hidden).unwrap();
        fs::write(hidden.join("secret.py"), "").unwrap();
        fs::write(root.join("visible.py"), "").unwrap();

        let meta = make_metadata(root.to_path_buf(), root.to_path_buf(), vec![]);
        let walker = ProjectFilesWalker::new(&meta);
        let paths = walker.collect_paths();
        let names: Vec<String> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"visible.py".to_string()));
        assert!(!names.contains(&"secret.py".to_string()));
    }

    // ── find_alembic_env_files ───────────────────────────────────────────

    #[test]
    fn test_find_alembic_env_direct() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let alembic = root.join("alembic");
        fs::create_dir(&alembic).unwrap();
        fs::write(alembic.join("env.py"), "# alembic env").unwrap();

        let results = find_alembic_env_files(root);
        assert!(!results.is_empty());
        assert!(results[0].ends_with("env.py"));
    }

    #[test]
    fn test_find_alembic_env_in_src() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let migrations = root.join("src").join("migrations");
        fs::create_dir_all(&migrations).unwrap();
        fs::write(migrations.join("env.py"), "# migrations env").unwrap();

        let results = find_alembic_env_files(root);
        assert!(!results.is_empty());
    }

    #[test]
    fn test_find_alembic_env_nested() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Nested: myapp/alembic/env.py (two levels deep from repo root)
        let nested = root.join("myapp").join("alembic");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("env.py"), "# nested env").unwrap();

        let results = find_alembic_env_files(root);
        assert!(!results.is_empty());
    }

    #[test]
    fn test_find_alembic_env_none() {
        let tmp = tempfile::tempdir().unwrap();
        let results = find_alembic_env_files(tmp.path());
        assert!(results.is_empty());
    }

    #[test]
    fn test_find_alembic_env_deduplicates() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let alembic = root.join("alembic");
        fs::create_dir(&alembic).unwrap();
        fs::write(alembic.join("env.py"), "# env").unwrap();

        let results = find_alembic_env_files(root);
        // The same file may be found by both the fast path and the walker.
        // Either way, results should be deduplicated.
        let unique: std::collections::HashSet<_> = results.iter().collect();
        assert_eq!(unique.len(), results.len());
    }

    // ── load_project_modules ─────────────────────────────────────────────

    #[test]
    fn test_load_project_modules_reads_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("main.py"), "app = FastAPI()").unwrap();
        fs::write(root.join("utils.py"), "def helper(): pass").unwrap();

        let meta = make_metadata(root.to_path_buf(), root.to_path_buf(), vec![]);
        let loaded = load_project_modules(meta).unwrap();

        assert_eq!(loaded.modules.len(), 2);
        // Modules should have relative paths and source content
        for module in &loaded.modules {
            assert!(module.rel_path.ends_with(".py"));
            assert!(!module.source.is_empty());
        }
    }

    #[test]
    fn test_load_project_modules_empty_project() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = make_metadata(tmp.path().to_path_buf(), tmp.path().to_path_buf(), vec![]);
        let loaded = load_project_modules(meta).unwrap();
        assert!(loaded.modules.is_empty());
    }

    #[test]
    fn test_load_project_modules_relative_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let sub = root.join("myapp");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("core.py"), "x = 1").unwrap();

        let meta = make_metadata(root.to_path_buf(), sub.clone(), vec![]);
        let loaded = load_project_modules(meta).unwrap();

        assert_eq!(loaded.modules.len(), 1);
        // rel_path should be relative to repo_root, using forward slashes
        assert_eq!(loaded.modules[0].rel_path, "myapp/core.py");
    }
}

fn collect_alembic_env_files(repo_root: &Path, out: &mut Vec<PathBuf>) {
    for root in [
        repo_root.to_path_buf(),
        repo_root.join("backend"),
        repo_root.join("src"),
    ] {
        if !root.is_dir() {
            continue;
        }

        for dirname in ["alembic", "migrations"] {
            let direct = root.join(dirname).join("env.py");
            if direct.is_file() {
                out.push(direct);
            }
        }

        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !matches!(name, "alembic" | "migrations") {
                continue;
            }
            let candidate = path.join("env.py");
            if candidate.is_file() {
                out.push(candidate);
            }
        }
    }
}
