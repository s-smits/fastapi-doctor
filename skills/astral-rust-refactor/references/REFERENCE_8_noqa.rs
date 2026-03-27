// Reference snapshot for inline-ignore / noqa semantics.
// Source repository: astral-sh/ruff
// Source file: crates/ruff_linter/src/noqa.rs
// Retrieved from `main` on 2026-03-27.

#[derive(Debug)]
pub(crate) enum Directive<'a> {
    All(All),
    Codes(Codes<'a>),
}

#[derive(Debug)]
pub(crate) struct All {
    range: TextRange,
}

impl Ranged for All {
    fn range(&self) -> TextRange {
        self.range
    }
}

#[derive(Debug)]
pub(crate) struct Code<'a> {
    code: &'a str,
    range: TextRange,
}

impl<'a> Code<'a> {
    pub(crate) fn as_str(&self) -> &'a str {
        self.code
    }
}

impl Display for Code<'_> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt.write_str(self.code)
    }
}

impl Ranged for Code<'_> {
    fn range(&self) -> TextRange {
        self.range
    }
}

#[derive(Debug)]
pub(crate) struct Codes<'a> {
    range: TextRange,
    codes: Vec<Code<'a>>,
}

impl Codes<'_> {
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, Code<'_>> {
        self.codes.iter()
    }

    pub(crate) fn includes<T: for<'a> PartialEq<&'a str>>(&self, needle: &T) -> bool {
        self.iter()
            .any(|code| *needle == get_redirect_target(code.as_str()).unwrap_or(code.as_str()))
    }
}

impl Ranged for Codes<'_> {
    fn range(&self) -> TextRange {
        self.range
    }
}

pub(crate) fn rule_is_ignored(
    code: Rule,
    offset: TextSize,
    noqa_line_for: &NoqaMapping,
    comment_ranges: &CommentRanges,
    locator: &Locator,
) -> bool {
    let offset = noqa_line_for.resolve(offset);
    let line_range = locator.line_range(offset);
    let &[comment_range] = comment_ranges.comments_in_range(line_range) else {
        return false;
    };
    match lex_inline_noqa(comment_range, locator.contents()) {
        Ok(Some(NoqaLexerOutput {
            directive: Directive::All(_),
            ..
        })) => true,
        Ok(Some(NoqaLexerOutput {
            directive: Directive::Codes(codes),
            ..
        })) => codes.includes(&code.noqa_code()),
        _ => false,
    }
}

#[derive(Debug)]
pub(crate) enum FileExemption {
    All(Vec<Rule>),
    Codes(Vec<Rule>),
}

impl FileExemption {
    pub(crate) fn contains_secondary_code(&self, needle: &SecondaryCode) -> bool {
        match self {
            FileExemption::All(_) => true,
            FileExemption::Codes(codes) => codes.iter().any(|code| *needle == code.noqa_code()),
        }
    }

    pub(crate) fn includes(&self, needle: Rule) -> bool {
        match self {
            FileExemption::All(_) => true,
            FileExemption::Codes(codes) => codes.contains(&needle),
        }
    }

    pub(crate) fn enumerates(&self, needle: Rule) -> bool {
        let codes = match self {
            FileExemption::All(codes) => codes,
            FileExemption::Codes(codes) => codes,
        };
        codes.contains(&needle)
    }
}
