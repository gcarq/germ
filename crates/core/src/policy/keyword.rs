use crate::deps::atom::Atom;
use crate::files::entry::Operation;
use crate::files::pkgfile::{KeywordRule, PackageAcceptKeywords};
use crate::keyword::{Keyword, KeywordSelector};
use crate::package::PackageView;

/// Immutable runtime policy that determines whether a package is accepted based on its keywords.
pub struct EffectiveKeywords {
    accept_keywords: Vec<KeywordSelector>,
    rules: Vec<(Atom, KeywordAction)>,
}

impl EffectiveKeywords {
    /// Builds [`EffectiveKeywords`] from keyword records.
    pub fn new(
        accept_keywords: Vec<KeywordSelector>,
        package_accept_keywords: PackageAcceptKeywords,
    ) -> Self {
        let mut rules = package_accept_keywords
            .into_rules()
            .flat_map(|(atom, rule)| {
                let prec = rule.precedence();
                KeywordAction::from_rule(rule, &accept_keywords)
                    .into_iter()
                    .map(move |action| (prec, atom.clone(), action))
            })
            .collect::<Vec<_>>();
        rules.sort_by_key(|(precedence, _, _)| *precedence);

        Self {
            accept_keywords,
            rules: rules
                .into_iter()
                .map(|(_, atom, action)| (atom, action))
                .collect(),
        }
    }

    /// Evaluates whether the given [`PackageView`] is accepted for the effective keywords.
    pub fn evaluate<P: PackageView>(&self, pkg: &P) -> KeywordEvalResult {
        if !self.keywords_accepted(pkg, &pkg.metadata().keywords) {
            return KeywordEvalResult::new(false, false);
        }

        // PMS 5.2.11: Stable restrictions “stable keyword in use” are applied exactly if
        //             replacing all stable keywords in `KEYWORDS` by the corresponding
        //             tilde prefixed keywords would result in the package installation
        //             being prevented due to the KEYWORDS setting.
        let testing_only = pkg
            .metadata()
            .keywords
            .iter()
            .map(|keyword| match keyword {
                Keyword::Stable(arch) => Keyword::Testing(arch.clone()),
                keyword => keyword.clone(),
            })
            .collect::<Vec<_>>();

        KeywordEvalResult::new(true, !self.keywords_accepted(pkg, &testing_only))
    }

    /// Returns `true` if the given [`PackageView`] accepts the given `keywords`.
    fn keywords_accepted<P: PackageView>(&self, pkg: &P, keywords: &[Keyword]) -> bool {
        if keywords.is_empty() {
            return self.evaluate_keyword(pkg, None);
        }

        keywords
            .iter()
            .any(|keyword| self.evaluate_keyword(pkg, Some(keyword)))
    }

    /// Evaluates whether the given `keyword` is accepted for the given [`PackageView`].
    fn evaluate_keyword<P: PackageView>(&self, pkg: &P, keyword: Option<&Keyword>) -> bool {
        let mut accepted = self
            .accept_keywords
            .iter()
            .any(|selector| selector.matches(keyword));

        for (atom, action) in &self.rules {
            if pkg.matches_atom(atom) {
                action.apply(keyword, &mut accepted);
            }
        }

        accepted
    }
}

/// Holds the result of evaluating a [`PackageView`]
/// against the effective keywords policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeywordEvalResult {
    pub accepted: bool,
    pub stable_in_use: bool,
}

impl KeywordEvalResult {
    pub const fn new(accepted: bool, stable_in_use: bool) -> Self {
        Self {
            accepted,
            stable_in_use,
        }
    }
}

/// Defines a compiled keyword rule that can be used to check whether a keyword is accepted.
enum KeywordAction {
    Reset,
    Accept(KeywordSelector),
    Reject(KeywordSelector),
}

impl KeywordAction {
    /// Creates one or more action from the given `rule`.
    ///
    /// For [`KeywordRule::AtomOnly`] the selectors are used to generate
    /// an unstable entry for each stable selector.
    fn from_rule(rule: KeywordRule, selectors: &[KeywordSelector]) -> Vec<Self> {
        match rule {
            KeywordRule::Selector(entry) => match entry.op {
                Operation::Set => vec![Self::Accept(entry.into_inner())],
                Operation::Unset => vec![Self::Reject(entry.into_inner())],
            },
            KeywordRule::Reset(_) => vec![Self::Reset],
            KeywordRule::AtomOnly(_) => selectors
                .iter()
                .filter_map(|sel| match sel {
                    KeywordSelector::Stable(arch) => {
                        Some(Self::Accept(KeywordSelector::Testing(arch.clone())))
                    }
                    KeywordSelector::AnyStable => Some(Self::Accept(KeywordSelector::AnyTesting)),
                    _ => None,
                })
                .collect(),
        }
    }

    fn apply(&self, keyword: Option<&Keyword>, accepted: &mut bool) {
        match self {
            Self::Reset => *accepted = false,
            Self::Accept(selector) if selector.matches(keyword) => *accepted = true,
            Self::Reject(selector) if selector.matches(keyword) => *accepted = false,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::entry::Precedence;
    use crate::package::Package;
    use crate::package::metadata::PackageMetadata;
    use crate::test_support::cpv;
    use crate::utils::Inherit;
    use crate::vdb::package::InstalledPackage;

    fn policy(accept_keywords: &str, package_keywords: &str) -> EffectiveKeywords {
        EffectiveKeywords::new(
            accept_keywords
                .split_whitespace()
                .map(str::parse)
                .collect::<anyhow::Result<_>>()
                .unwrap(),
            PackageAcceptKeywords::from_string(package_keywords.into(), Precedence::User).unwrap(),
        )
    }

    fn package<'a>(keywords: impl IntoIterator<Item = &'a str>) -> Package {
        Package::new(
            cpv("dev-lang", "rust", "1.0"),
            "gentoo".parse().unwrap(),
            PackageMetadata {
                keywords: keywords
                    .into_iter()
                    .map(str::parse)
                    .collect::<anyhow::Result<_>>()
                    .unwrap(),
                ..Default::default()
            },
        )
    }

    #[test]
    fn test_policy_global_keywords() {
        let policy = policy("amd64 ~arm64", "");
        let test_cases = [
            ("amd64", KeywordEvalResult::new(true, true)),
            ("~arm64", KeywordEvalResult::new(true, false)),
            ("~amd64", KeywordEvalResult::new(false, false)),
        ];

        for (keyword, expected) in test_cases {
            let pkg = package([keyword]);
            assert_eq!(policy.evaluate(&pkg), expected);
        }
    }

    #[test]
    fn test_policy_global_wildcards() {
        let test_cases = &[
            ("*", "amd64", KeywordEvalResult::new(true, true)),
            ("*", "~amd64", KeywordEvalResult::new(false, false)),
            ("~*", "~amd64", KeywordEvalResult::new(true, false)),
            ("~*", "amd64", KeywordEvalResult::new(false, false)),
            ("**", "", KeywordEvalResult::new(true, false)),
            ("**", "-amd64", KeywordEvalResult::new(true, false)),
        ];

        for &(accept_keywords, package_keywords, expected) in test_cases {
            let policy = policy(accept_keywords, "");
            let pkg = package(package_keywords.split_whitespace());
            assert_eq!(policy.evaluate(&pkg), expected);
        }
    }

    #[test]
    fn test_policy_selector_reject() {
        let policy = policy("amd64", "dev-lang/rust -amd64");
        let pkg = package(["amd64"]);
        assert_eq!(policy.evaluate(&pkg), KeywordEvalResult::new(false, false));
    }

    #[test]
    fn test_policy_reset_accept() {
        let policy = policy("amd64", "dev-lang/rust -* ~amd64");
        let pkg = package(["~amd64"]);
        assert_eq!(policy.evaluate(&pkg), KeywordEvalResult::new(true, false));
    }

    #[test]
    fn test_policy_atom_only() {
        let test_cases = [
            ("amd64", "~amd64", KeywordEvalResult::new(true, false)),
            ("amd64", "~arm64", KeywordEvalResult::new(false, false)),
            ("*", "~arm64", KeywordEvalResult::new(true, false)),
        ];

        for (accept_keywords, package_keywords, expected) in test_cases {
            let pkg = package([package_keywords]);
            let keywords = policy(accept_keywords, "dev-lang/rust");
            assert_eq!(keywords.evaluate(&pkg), expected);
        }
    }

    #[test]
    fn test_policy_atom_order() {
        let test_cases = [
            (
                "*/* -~amd64
                dev-lang/rust ~amd64",
                KeywordEvalResult::new(true, false),
            ),
            (
                "dev-lang/rust ~amd64
                */* -~amd64",
                KeywordEvalResult::new(false, false),
            ),
        ];

        let pkg = package(["~amd64"]);
        for (package_keywords, expected) in test_cases {
            let keywords = policy("", package_keywords);
            assert_eq!(keywords.evaluate(&pkg), expected);
        }
    }

    #[test]
    fn test_policy_precedence() -> anyhow::Result<()> {
        let profile = PackageAcceptKeywords::from_string(
            "dev-lang/rust amd64".into(),
            Precedence::Profile(0),
        )?;
        let user = PackageAcceptKeywords::from_string("*/* -amd64".into(), Precedence::User)?;
        let package_accept_keywords = user.inherit(&profile)?;
        let policy = EffectiveKeywords::new(Vec::default(), package_accept_keywords);

        let pkg = package(["amd64"]);
        assert_eq!(policy.evaluate(&pkg), KeywordEvalResult::new(false, false));
        Ok(())
    }

    #[test]
    fn test_policy_any() {
        let policy = policy("", "dev-lang/rust **");
        let test_cases = [
            ("", KeywordEvalResult::new(true, false)),
            ("-amd64", KeywordEvalResult::new(true, false)),
            ("-*", KeywordEvalResult::new(true, false)),
        ];

        for (package_keywords, expected) in test_cases {
            let pkg = package(package_keywords.split_whitespace());
            assert_eq!(policy.evaluate(&pkg), expected);
        }
    }

    #[test]
    fn test_policy_installed_package() {
        let policy = policy("amd64", "");
        let pkg = InstalledPackage {
            cpv: cpv("dev-lang", "rust", "1.0"),
            repo: "gentoo".parse().unwrap(),
            metadata: PackageMetadata {
                keywords: vec!["amd64".parse().unwrap()],
                ..Default::default()
            },
            use_flags: Vec::new(),
        };
        assert_eq!(policy.evaluate(&pkg), KeywordEvalResult::new(true, true));
    }

    #[test]
    fn test_policy_stable_keywords() {
        let keywords = policy("amd64 ~amd64", "");
        let pkg = package(["amd64"]);
        assert_eq!(keywords.evaluate(&pkg), KeywordEvalResult::new(true, false));

        let keywords = policy("amd64 ~arm64", "");
        let pkg = package(["amd64", "arm64"]);
        assert_eq!(keywords.evaluate(&pkg), KeywordEvalResult::new(true, false));

        let keywords = policy("amd64", "dev-lang/rust ~amd64");
        let pkg = package(["amd64"]);
        assert_eq!(keywords.evaluate(&pkg), KeywordEvalResult::new(true, false));
    }
}
