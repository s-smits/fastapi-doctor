// Reference snapshot for parsed-module wrapper and parser facade.
// Source repository: astral-sh/ruff
// Source file: crates/ruff_db/src/parsed.rs
// Retrieved from `main` on 2026-03-27.

#[salsa::tracked(returns(ref), no_eq, heap_size=ruff_memory_usage::heap_size, lru=200)]
pub fn parsed_module(db: &dyn Db, file: File) -> ParsedModule {
    let _span = tracing::trace_span!("parsed_module", ?file).entered();

    let parsed = parsed_module_impl(db, file);

    ParsedModule::new(file, parsed)
}

pub fn parsed_module_impl(db: &dyn Db, file: File) -> Parsed<ModModule> {
    let source = source_text(db, file);
    let ty = file.source_type(db);

    let target_version = db.python_version();
    let options = ParseOptions::from(ty).with_target_version(target_version);
    parse_unchecked(&source, options)
        .try_into_module()
        .expect("PySourceType always parses into a module")
}

#[derive(Clone, get_size2::GetSize)]
pub struct ParsedModule {
    file: File,
    #[get_size(size_fn = arc_swap_size)]
    inner: Arc<ArcSwapOption<indexed::IndexedModule>>,
}

impl ParsedModule {
    pub fn new(file: File, parsed: Parsed<ModModule>) -> Self {
        Self {
            file,
            inner: Arc::new(ArcSwapOption::new(Some(indexed::IndexedModule::new(
                parsed,
            )))),
        }
    }

    pub fn load(&self, db: &dyn Db) -> ParsedModuleRef {
        let parsed = match self.inner.load_full() {
            Some(parsed) => parsed,
            None => {
                let parsed = indexed::IndexedModule::new(parsed_module_impl(db, self.file));
                tracing::debug!(
                    "File `{}` was reparsed after being collected in the current Salsa revision",
                    self.file.path(db)
                );

                self.inner.store(Some(parsed.clone()));
                parsed
            }
        };

        ParsedModuleRef {
            module: self.clone(),
            indexed: parsed,
        }
    }

    pub fn clear(&self) {
        self.inner.store(None);
    }

    pub fn file(&self) -> File {
        self.file
    }
}

#[derive(Clone)]
pub struct ParsedModuleRef {
    module: ParsedModule,
    indexed: Arc<indexed::IndexedModule>,
}

impl ParsedModuleRef {
    pub fn module(&self) -> &ParsedModule {
        &self.module
    }

    pub fn get_by_index<'ast>(&'ast self, index: NodeIndex) -> AnyRootNodeRef<'ast> {
        self.indexed.get_by_index(index)
    }
}

impl std::ops::Deref for ParsedModuleRef {
    type Target = Parsed<ModModule>;

    fn deref(&self) -> &Self::Target {
        &self.indexed.parsed
    }
}

fn arc_swap_size<T>(arc_swap: &Arc<ArcSwapOption<T>>) -> usize
where
    T: GetSize,
{
    if let Some(value) = &*arc_swap.load() {
        T::get_heap_size(value)
    } else {
        0
    }
}
