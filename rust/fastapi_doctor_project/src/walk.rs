use std::fs;
use std::path::{Path, PathBuf};

use fastapi_doctor_core::{path_to_string, ModuleRecord};
use rayon::prelude::*;

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
