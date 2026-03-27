use std::path::PathBuf;

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
}
