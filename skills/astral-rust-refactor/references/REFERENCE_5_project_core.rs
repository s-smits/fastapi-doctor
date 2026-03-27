// Reference snapshot for the project model and rule access shape.
// Source repository: astral-sh/ruff
// Source file: crates/ty_project/src/lib.rs
// Retrieved from `main` on 2026-03-27.

#[salsa::input(heap_size=ruff_memory_usage::heap_size)]
#[derive(Debug)]
pub struct Project {
    #[returns(ref)]
    #[default]
    open_fileset: FxHashSet<File>,

    #[default]
    #[returns(ref)]
    file_set: IndexedFiles,

    #[returns(deref)]
    pub metadata: Box<ProjectMetadata>,

    #[returns(deref)]
    pub settings: Box<Settings>,

    #[default]
    #[returns(deref)]
    included_paths_list: Vec<SystemPathBuf>,

    #[returns(deref)]
    settings_diagnostics: Vec<OptionDiagnostic>,

    #[default]
    check_mode: CheckMode,

    #[default]
    verbose_flag: bool,

    #[default]
    force_exclude_flag: bool,
}

#[salsa::tracked]
impl Project {
    pub fn from_metadata<Strategy: MisconfigurationStrategy>(
        db: &dyn Db,
        metadata: ProjectMetadata,
        strategy: &Strategy,
    ) -> Result<Self, Strategy::Error<ToSettingsError>> {
        let (settings, diagnostics) =
            metadata
                .options()
                .to_settings(db, metadata.root(), strategy)?;

        let project = Project::builder(Box::new(metadata), Box::new(settings), diagnostics)
            .durability(Durability::MEDIUM)
            .open_fileset_durability(Durability::LOW)
            .file_set_durability(Durability::LOW)
            .new(db);

        project.try_add_file_root(db);

        Ok(project)
    }

    fn try_add_file_root(self, db: &dyn Db) {
        db.files()
            .try_add_root(db, self.root(db), FileRootKind::Project);
    }

    pub fn root(self, db: &dyn Db) -> &SystemPath {
        self.metadata(db).root()
    }

    pub fn name(self, db: &dyn Db) -> &str {
        self.metadata(db).name()
    }

    #[salsa::tracked(returns(deref), heap_size=ruff_memory_usage::heap_size)]
    pub fn rules(self, db: &dyn Db) -> Arc<RuleSelection> {
        self.settings(db).to_rules()
    }

    pub fn is_file_included(self, db: &dyn Db, path: &SystemPath) -> bool {
        matches!(
            ProjectFilesFilter::from_project(db, self)
                .is_file_included(path, GlobFilterCheckMode::Adhoc),
            IncludeResult::Included { .. }
        )
    }

    pub fn is_directory_included(self, db: &dyn Db, path: &SystemPath) -> bool {
        matches!(
            ProjectFilesFilter::from_project(db, self)
                .is_directory_included(path, GlobFilterCheckMode::Adhoc),
            IncludeResult::Included { .. }
        )
    }
}
