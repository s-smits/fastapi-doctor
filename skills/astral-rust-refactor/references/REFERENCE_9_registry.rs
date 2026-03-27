// Reference snapshot for rule registry traits and source classification.
// Source repository: astral-sh/ruff
// Source file: crates/ruff_linter/src/registry.rs
// Retrieved from `main` on 2026-03-27.

pub trait AsRule {
    fn rule(&self) -> Rule;
}

impl Rule {
    pub fn from_code(code: &str) -> Result<Self, FromCodeError> {
        let (linter, code) = Linter::parse_code(code).ok_or(FromCodeError::Unknown)?;
        linter
            .all_rules()
            .find(|rule| rule.noqa_code().suffix() == code)
            .ok_or(FromCodeError::Unknown)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum FromCodeError {
    #[error("unknown rule code")]
    Unknown,
}

pub trait RuleNamespace: Sized {
    fn common_prefix(&self) -> &'static str;
    fn parse_code(code: &str) -> Option<(Self, &str)>;
    fn name(&self) -> &'static str;
    fn url(&self) -> Option<&'static str>;
}

#[derive(is_macro::Is, Copy, Clone)]
pub enum LintSource {
    Ast,
    Io,
    PhysicalLines,
    LogicalLines,
    Tokens,
    Imports,
    Noqa,
    Filesystem,
    PyprojectToml,
}

impl Rule {
    pub const fn lint_source(&self) -> LintSource {
        match self {
            Rule::InvalidPyprojectToml => LintSource::PyprojectToml,
            Rule::BlanketNOQA | Rule::RedirectedNOQA | Rule::UnusedNOQA => LintSource::Noqa,
            Rule::BidirectionalUnicode
            | Rule::BlankLineWithWhitespace
            | Rule::DocLineTooLong
            | Rule::IndentedFormFeed
            | Rule::LineTooLong
            | Rule::MissingCopyrightNotice
            | Rule::MissingNewlineAtEndOfFile
            | Rule::MixedSpacesAndTabs => LintSource::PhysicalLines,
            _ => LintSource::Ast,
        }
    }
}
