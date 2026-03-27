// Reference snapshot for structured suppression handling.
// Source repository: astral-sh/ruff
// Source file: crates/ruff_linter/src/suppression.rs
// Retrieved from `main` on 2026-03-27.

#[derive(Clone, Debug, Eq, PartialEq)]
enum SuppressionAction {
    Disable,
    Enable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SuppressionComment {
    range: TextRange,
    action: SuppressionAction,
    codes: SmallVec<[TextRange; 2]>,
    reason: TextRange,
}

impl SuppressionComment {
    fn codes_as_str<'src>(&self, source: &'src str) -> impl Iterator<Item = &'src str> {
        self.codes.iter().map(|range| source.slice(range))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PendingSuppressionComment<'a> {
    indent: &'a str,
    comment: SuppressionComment,
}

impl PendingSuppressionComment<'_> {
    fn matches(&self, other: &PendingSuppressionComment, source: &str) -> bool {
        self.comment.action == SuppressionAction::Disable
            && other.comment.action == SuppressionAction::Enable
            && self.indent == other.indent
            && self
                .comment
                .codes_as_str(source)
                .eq(other.comment.codes_as_str(source))
    }
}

#[derive(Debug)]
pub(crate) struct Suppression {
    code: CompactString,
    range: TextRange,
    used: Cell<bool>,
    comments: DisableEnableComments,
}

impl Suppression {
    fn codes(&self) -> &[TextRange] {
        &self.comments.disable_comment().codes
    }
}

#[derive(Debug)]
pub(crate) enum DisableEnableComments {
    Disable(SuppressionComment),
    DisableEnable(SuppressionComment, SuppressionComment),
}

impl DisableEnableComments {
    pub(crate) fn disable_comment(&self) -> &SuppressionComment {
        match self {
            DisableEnableComments::Disable(comment) => comment,
            DisableEnableComments::DisableEnable(disable, _) => disable,
        }
    }
    pub(crate) fn enable_comment(&self) -> Option<&SuppressionComment> {
        match self {
            DisableEnableComments::Disable(_) => None,
            DisableEnableComments::DisableEnable(_, enable) => Some(enable),
        }
    }
}

#[derive(Debug, Default)]
pub struct Suppressions {
    valid: Vec<Suppression>,
    invalid: Vec<InvalidSuppression>,
    errors: Vec<ParseError>,
}

impl Suppressions {
    pub fn from_tokens(source: &str, tokens: &Tokens, indexer: &Indexer) -> Suppressions {
        let builder = SuppressionsBuilder::new(source);
        builder.load_from_tokens(tokens, indexer)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.valid.is_empty() && self.invalid.is_empty() && self.errors.is_empty()
    }

    pub(crate) fn check_diagnostic(&self, diagnostic: &Diagnostic) -> bool {
        if self.valid.is_empty() {
            return false;
        }

        let Some(code) = diagnostic.secondary_code() else {
            return false;
        };
        let Some(span) = diagnostic.primary_span() else {
            return false;
        };
        let Some(range) = span.range() else {
            return false;
        };

        for suppression in &self.valid {
            let suppression_code =
                get_redirect_target(suppression.code.as_str()).unwrap_or(suppression.code.as_str());
            if *code == suppression_code && suppression.range.contains_range(range) {
                suppression.used.set(true);
                return true;
            }
        }
        false
    }
}
