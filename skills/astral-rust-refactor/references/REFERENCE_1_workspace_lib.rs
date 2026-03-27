// Reference snapshot for crate-root shape.
// Source repository: astral-sh/ruff
// Source file: crates/ruff_workspace/src/lib.rs
// Retrieved from `main` on 2026-03-27.

pub mod configuration;
pub mod options;
pub mod pyproject;
pub mod resolver;

mod settings;

pub use settings::{FileResolverSettings, FormatterSettings, Settings};

#[cfg(test)]
mod tests {
    use std::path::Path;

    pub(crate) fn test_resource_path(path: impl AsRef<Path>) -> std::path::PathBuf {
        Path::new("../ruff_linter/resources/test/").join(path)
    }
}
