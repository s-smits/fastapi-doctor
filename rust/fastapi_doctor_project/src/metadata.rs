use std::path::PathBuf;

use crate::ProjectContext;

#[derive(Debug, Clone)]
pub struct ProjectMetadata {
    pub repo_root: PathBuf,
    pub code_root: PathBuf,
    pub excluded_dirs: Vec<String>,
}

impl ProjectMetadata {
    pub fn new(repo_root: PathBuf, code_root: PathBuf, excluded_dirs: Vec<String>) -> Self {
        Self {
            repo_root,
            code_root,
            excluded_dirs,
        }
    }

    pub fn from_context(context: &ProjectContext) -> Self {
        Self {
            repo_root: context.layout.repo_root.clone(),
            code_root: context.layout.code_dir.clone(),
            excluded_dirs: context.effective_config.scan.exclude_dirs.clone(),
        }
    }
}
