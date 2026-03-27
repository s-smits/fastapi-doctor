// Reference snapshot for workspace resolver and scoped settings.
// Source repository: astral-sh/ruff
// Source file: crates/ruff_workspace/src/resolver.rs
// Retrieved from `main` on 2026-03-27.

#[derive(Debug)]
pub struct PyprojectConfig {
    /// The strategy used to discover the relevant `pyproject.toml` file for
    /// each Python file.
    pub strategy: PyprojectDiscoveryStrategy,
    /// All settings from the `pyproject.toml` file.
    pub settings: Settings,
    /// Absolute path to the `pyproject.toml` file. This would be `None` when
    /// either using the default settings or the `--isolated` flag is set.
    pub path: Option<PathBuf>,
}

impl PyprojectConfig {
    pub fn new(
        strategy: PyprojectDiscoveryStrategy,
        settings: Settings,
        path: Option<PathBuf>,
    ) -> Self {
        Self {
            strategy,
            settings,
            path: path.map(fs::normalize_path),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum PyprojectDiscoveryStrategy {
    Fixed,
    Hierarchical,
}

impl PyprojectDiscoveryStrategy {
    #[inline]
    pub const fn is_fixed(self) -> bool {
        matches!(self, PyprojectDiscoveryStrategy::Fixed)
    }

    #[inline]
    pub const fn is_hierarchical(self) -> bool {
        matches!(self, PyprojectDiscoveryStrategy::Hierarchical)
    }
}

#[derive(Copy, Clone)]
pub enum Relativity {
    Cwd,
    Parent,
}

impl Relativity {
    pub fn resolve(self, path: &Path) -> &Path {
        match self {
            Relativity::Parent => path
                .parent()
                .expect("Expected pyproject.toml file to be in parent directory"),
            Relativity::Cwd => &path_dedot::CWD,
        }
    }
}

#[derive(Debug)]
pub struct Resolver<'a> {
    pyproject_config: &'a PyprojectConfig,
    settings: Vec<(Settings, PathBuf)>,
    router: Router<usize>,
}

impl<'a> Resolver<'a> {
    pub fn new(pyproject_config: &'a PyprojectConfig) -> Self {
        Self {
            pyproject_config,
            settings: Vec::new(),
            router: Router::new(),
        }
    }

    #[inline]
    pub fn base_settings(&self) -> &Settings {
        &self.pyproject_config.settings
    }

    #[inline]
    pub fn is_hierarchical(&self) -> bool {
        self.pyproject_config.strategy.is_hierarchical()
    }

    #[inline]
    pub fn force_exclude(&self) -> bool {
        self.pyproject_config.settings.file_resolver.force_exclude
    }

    #[inline]
    pub fn respect_gitignore(&self) -> bool {
        self.pyproject_config
            .settings
            .file_resolver
            .respect_gitignore
    }

    fn add(&mut self, path: &Path, settings: Settings, config_path: PathBuf) {
        self.settings.push((settings, config_path));

        let path = path.to_slash_lossy().replace('{', "{{").replace('}', "}}");

        match self
            .router
            .insert(format!("{path}/{{*filepath}}"), self.settings.len() - 1)
        {
            Ok(()) => {}
            Err(InsertError::Conflict { .. }) => {
                return;
            }
            Err(_) => unreachable!("file paths are escaped before being inserted in the router"),
        }

        self.router.insert(path, self.settings.len() - 1).unwrap();
    }

    pub fn resolve(&self, path: &Path) -> &Settings {
        self.resolve_with_path(path).0
    }

    pub fn resolve_with_path(&self, path: &Path) -> (&Settings, Option<&Path>) {
        match self.pyproject_config.strategy {
            PyprojectDiscoveryStrategy::Fixed => (
                &self.pyproject_config.settings,
                self.pyproject_config.path.as_deref(),
            ),
            PyprojectDiscoveryStrategy::Hierarchical => self
                .router
                .at(path.to_slash_lossy().as_ref())
                .map(|Match { value, .. }| {
                    let (settings, config_path) = &self.settings[*value];
                    (settings, Some(config_path.as_path()))
                })
                .unwrap_or((
                    &self.pyproject_config.settings,
                    self.pyproject_config.path.as_deref(),
                )),
        }
    }
}
