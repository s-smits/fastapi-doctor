// Reference snapshot for rule-selector parsing and exact-vs-prefix semantics.
// Source repository: astral-sh/ruff
// Source file: crates/ruff_linter/src/rule_selector.rs
// Retrieved from `main` on 2026-03-27.

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RuleSelector {
    All,
    C,
    T,
    Linter(Linter),
    Prefix {
        prefix: RuleCodePrefix,
        redirected_from: Option<&'static str>,
    },
    Rule {
        prefix: RuleCodePrefix,
        redirected_from: Option<&'static str>,
    },
}

impl RuleSelector {
    pub(crate) const fn rule(prefix: RuleCodePrefix) -> Self {
        Self::Rule {
            prefix,
            redirected_from: None,
        }
    }
}

impl From<Linter> for RuleSelector {
    fn from(linter: Linter) -> Self {
        Self::Linter(linter)
    }
}

pub(crate) fn is_single_rule_selector(prefix: &RuleCodePrefix) -> bool {
    let mut rules = prefix.rules();

    let Some(rule) = rules.next() else {
        return false;
    };
    if rules.next().is_some() {
        return false;
    }

    rule.noqa_code().suffix() == prefix.short_code()
}

impl FromStr for RuleSelector {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ALL" => Ok(Self::All),
            "C" => Ok(Self::C),
            "T" => Ok(Self::T),
            _ => {
                let (s, redirected_from) = match get_redirect(s) {
                    Some((from, target)) => (target, Some(from)),
                    None => (s, None),
                };

                let (linter, code) =
                    Linter::parse_code(s).ok_or_else(|| ParseError::Unknown(s.to_string()))?;

                if code.is_empty() {
                    return Ok(Self::Linter(linter));
                }

                let prefix = RuleCodePrefix::parse(&linter, code)
                    .map_err(|_| ParseError::Unknown(s.to_string()))?;

                if is_single_rule_selector(&prefix) {
                    Ok(Self::Rule {
                        prefix,
                        redirected_from,
                    })
                } else {
                    Ok(Self::Prefix {
                        prefix,
                        redirected_from,
                    })
                }
            }
        }
    }
}

impl RuleSelector {
    pub fn prefix_and_code(&self) -> (&'static str, &'static str) {
        match self {
            RuleSelector::All => ("", "ALL"),
            RuleSelector::C => ("", "C"),
            RuleSelector::T => ("", "T"),
            RuleSelector::Prefix { prefix, .. } | RuleSelector::Rule { prefix, .. } => {
                (prefix.linter().common_prefix(), prefix.short_code())
            }
            RuleSelector::Linter(l) => (l.common_prefix(), ""),
        }
    }

    pub fn all_rules(&self) -> impl Iterator<Item = Rule> + '_ {
        match self {
            RuleSelector::All => RuleSelectorIter::All(Rule::iter()),
            RuleSelector::C => RuleSelectorIter::Chain(
                Linter::Flake8Comprehensions
                    .rules()
                    .chain(Linter::McCabe.rules()),
            ),
            RuleSelector::T => RuleSelectorIter::Chain(
                Linter::Flake8Debugger
                    .rules()
                    .chain(Linter::Flake8Print.rules()),
            ),
            RuleSelector::Linter(linter) => RuleSelectorIter::Vec(linter.rules()),
            RuleSelector::Prefix { prefix, .. } | RuleSelector::Rule { prefix, .. } => {
                RuleSelectorIter::Vec(prefix.clone().rules())
            }
        }
    }

    pub fn rules<'a>(&'a self, preview: &PreviewOptions) -> impl Iterator<Item = Rule> + use<'a> {
        let preview_enabled = preview.mode.is_enabled();
        let preview_require_explicit = preview.require_explicit;

        self.all_rules().filter(move |rule| {
            match rule.group() {
                RuleGroup::Stable { .. } => true,
                RuleGroup::Preview { .. } => {
                    preview_enabled && (self.is_exact() || !preview_require_explicit)
                }
                RuleGroup::Deprecated { .. } => !preview_enabled && self.is_exact(),
                RuleGroup::Removed { .. } => self.is_exact(),
            }
        })
    }

    pub fn is_exact(&self) -> bool {
        matches!(self, Self::Rule { .. })
    }
}
