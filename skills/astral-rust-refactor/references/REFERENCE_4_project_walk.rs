// Reference snapshot for project file filtering and walking.
// Source repository: astral-sh/ruff
// Source file: crates/ty_project/src/walk.rs
// Retrieved from `main` on 2026-03-27.

#[derive(Debug)]
pub(crate) struct ProjectFilesFilter<'a> {
    included_paths: &'a [SystemPathBuf],
    src_filter: &'a IncludeExcludeFilter,
    force_exclude: bool,
}

impl<'a> ProjectFilesFilter<'a> {
    pub(crate) fn from_project(db: &'a dyn Db, project: Project) -> Self {
        Self {
            included_paths: project.included_paths_or_root(db),
            src_filter: &project.settings(db).src().files,
            force_exclude: project.force_exclude(db),
        }
    }

    pub(crate) fn force_exclude(&self) -> bool {
        self.force_exclude
    }

    fn match_included_paths(
        &self,
        path: &SystemPath,
        mode: GlobFilterCheckMode,
    ) -> Option<CheckPathMatch> {
        match mode {
            GlobFilterCheckMode::TopDown => Some(CheckPathMatch::Partial),
            GlobFilterCheckMode::Adhoc => {
                self.included_paths
                    .iter()
                    .filter_map(|included_path| {
                        if let Ok(relative_path) = path.strip_prefix(included_path) {
                            if relative_path.as_str().is_empty() && !self.force_exclude {
                                Some(CheckPathMatch::Full)
                            } else {
                                Some(CheckPathMatch::Partial)
                            }
                        } else {
                            None
                        }
                    })
                    .max()
            }
        }
    }

    pub(crate) fn is_file_included(
        &self,
        path: &SystemPath,
        mode: GlobFilterCheckMode,
    ) -> IncludeResult {
        match self.match_included_paths(path, mode) {
            None => IncludeResult::NotIncluded,
            Some(CheckPathMatch::Partial) => self.src_filter.is_file_included(path, mode),
            Some(CheckPathMatch::Full) => IncludeResult::Included {
                literal_match: Some(true),
            },
        }
    }

    pub(crate) fn is_directory_included(
        &self,
        path: &SystemPath,
        mode: GlobFilterCheckMode,
    ) -> IncludeResult {
        match self.match_included_paths(path, mode) {
            None => IncludeResult::NotIncluded,
            Some(CheckPathMatch::Partial) => {
                self.src_filter.is_directory_maybe_included(path, mode)
            }
            Some(CheckPathMatch::Full) => IncludeResult::Included {
                literal_match: Some(true),
            },
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum CheckPathMatch {
    Partial,
    Full,
}

pub(crate) struct ProjectFilesWalker<'a> {
    walker: WalkDirectoryBuilder,
    filter: ProjectFilesFilter<'a>,
}

impl<'a> ProjectFilesWalker<'a> {
    pub(crate) fn new(db: &'a dyn Db) -> Self {
        let project = db.project();

        let filter = ProjectFilesFilter::from_project(db, project);

        Self::from_paths(db, project.included_paths_or_root(db), filter)
            .expect("included_paths_or_root to never return an empty iterator")
    }

    pub(crate) fn incremental<P>(db: &'a dyn Db, paths: impl IntoIterator<Item = P>) -> Option<Self>
    where
        P: AsRef<SystemPath>,
    {
        let project = db.project();

        let filter = ProjectFilesFilter::from_project(db, project);

        Self::from_paths(db, paths, filter)
    }

    fn from_paths<P>(
        db: &'a dyn Db,
        paths: impl IntoIterator<Item = P>,
        filter: ProjectFilesFilter<'a>,
    ) -> Option<Self>
    where
        P: AsRef<SystemPath>,
    {
        let mut paths = paths.into_iter();

        let mut walker = db
            .system()
            .walk_directory(paths.next()?.as_ref())
            .standard_filters(db.project().settings(db).src().respect_ignore_files)
            .ignore_hidden(false);

        for path in paths {
            walker = walker.add(path);
        }

        Some(Self { walker, filter })
    }
}
