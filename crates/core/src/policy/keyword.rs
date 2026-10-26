use germ_pms::{Keyword, PackageView};

use super::index::AtomIndex;
use crate::files::entry::Operation;
use crate::files::pkgfile::{KeywordRule, PackageAcceptKeywords};
use crate::keyword::KeywordSelector;

/// Immutable runtime policy that determines whether a package is accepted based on its keywords.
pub struct EffectiveKeywords {
    accept_keywords: Vec<KeywordSelector>,
    rules: AtomIndex<KeywordAction>,
}

impl EffectiveKeywords {
    /// Builds [`EffectiveKeywords`] from keyword records.
    pub fn new(
        accept_keywords: Vec<KeywordSelector>,
        package_accept_keywords: PackageAcceptKeywords,
    ) -> Self {
        let mut sorted = package_accept_keywords
            .into_rules()
            .flat_map(|(atom, rule)| {
                let prec = rule.precedence();
                KeywordAction::from_rule(rule, &accept_keywords)
                    .into_iter()
                    .map(move |action| (prec, atom.clone(), action))
            })
            .collect::<Vec<_>>();
        sorted.sort_by_key(|(precedence, _, _)| *precedence);

        let rules = sorted
            .into_iter()
            .map(|(_, atom, action)| (atom, action))
            .collect();
        Self {
            accept_keywords,
            rules: AtomIndex::new(rules),
        }
    }

    /// Evaluates whether the given [`PackageView`] is accepted for the effective keywords.
    pub fn eval<P: PackageView>(&self, pkg: &P) -> KeywordEvalResult {
        if !self.keywords_accepted(pkg, pkg.metadata().keywords()) {
            return KeywordEvalResult::Rejected;
        }

        // PMS 5.2.11: Stable restrictions “stable keyword in use” are applied exactly if
        //             replacing all stable keywords in `KEYWORDS` by the corresponding
        //             tilde prefixed keywords would result in the package installation
        //             being prevented due to the KEYWORDS setting.
        let testing_only = pkg
            .metadata()
            .keywords()
            .iter()
            .map(|keyword| match keyword {
                Keyword::Stable(arch) => Keyword::Testing(arch.clone()),
                keyword => keyword.clone(),
            })
            .collect::<Vec<_>>();

        let stable_in_use = !self.keywords_accepted(pkg, &testing_only);
        KeywordEvalResult::Accepted { stable_in_use }
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

        self.rules
            .visit_matches(pkg, |action| action.apply(keyword, &mut accepted));

        accepted
    }
}

/// Holds the result of evaluating a [`PackageView`]
/// against the effective keywords policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordEvalResult {
    Accepted { stable_in_use: bool },
    Rejected,
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
    use germ_pms::Package;
    use germ_pms::test_support::{cpv, pkg, pkg_metadata};

    use super::*;
    use crate::files::entry::Precedence;
    use crate::useflag::EffectiveUse;
    use crate::utils::Inherit;
    use crate::vdb::package::InstalledPackage;

    fn policy(accept_keywords: &str, package_keywords: &str) -> EffectiveKeywords {
        EffectiveKeywords::new(
            accept_keywords
                .split_whitespace()
                .map(str::parse)
                .collect::<anyhow::Result<_>>()
                .unwrap(),
            PackageAcceptKeywords::from_lines(package_keywords.lines(), Precedence::User).unwrap(),
        )
    }

    fn package<'a>(keywords: impl IntoIterator<Item = &'a str>) -> Package {
        pkg(
            "dev-lang/rust-1.0",
            &[(
                "KEYWORDS",
                &keywords.into_iter().collect::<Vec<_>>().join(" "),
            )],
        )
    }

    #[test]
    fn test_policy() {
        #[rustfmt::skip]
        let cases = [
            ("amd64 ~arm64", "", "amd64", KeywordEvalResult::Accepted { stable_in_use: true }),
            ("amd64 ~arm64", "", "~arm64", KeywordEvalResult::Accepted { stable_in_use: false }),
            ("amd64 ~arm64", "", "~amd64", KeywordEvalResult::Rejected),
            ("*", "", "amd64", KeywordEvalResult::Accepted { stable_in_use: true }),
            ("*", "", "~amd64", KeywordEvalResult::Rejected),
            ("~*", "", "~amd64", KeywordEvalResult::Accepted { stable_in_use: false }),
            ("~*", "", "amd64", KeywordEvalResult::Rejected),
            ("**", "", "", KeywordEvalResult::Accepted { stable_in_use: false }),
            ("**", "", "-amd64", KeywordEvalResult::Accepted { stable_in_use: false }),
            ("amd64", "dev-lang/rust -amd64", "amd64", KeywordEvalResult::Rejected),
            ("amd64", "dev-lang/rust -* ~amd64", "~amd64", KeywordEvalResult::Accepted { stable_in_use: false }),
            ("amd64", "dev-lang/rust", "~amd64", KeywordEvalResult::Accepted { stable_in_use: false }),
            ("amd64", "dev-lang/rust", "~arm64", KeywordEvalResult::Rejected),
            ("*", "dev-lang/rust", "~arm64", KeywordEvalResult::Accepted { stable_in_use: false }),
            ("", "dev-lang/rust **", "", KeywordEvalResult::Accepted { stable_in_use: false }),
            ("", "dev-lang/rust **", "-amd64", KeywordEvalResult::Accepted { stable_in_use: false }),
            ("", "dev-lang/rust **", "-*", KeywordEvalResult::Accepted { stable_in_use: false }),
        ];

        for (accept_keywords, package_accept_keywords, keywords, expected) in cases {
            let policy = policy(accept_keywords, package_accept_keywords);
            let pkg = package(keywords.split_whitespace());
            assert_eq!(policy.eval(&pkg), expected, "{keywords}");
        }
    }

    #[test]
    fn test_policy_atom_order() {
        let test_cases = [
            (
                "*/* -~amd64\ndev-lang/rust ~amd64",
                KeywordEvalResult::Accepted {
                    stable_in_use: false,
                },
            ),
            (
                "dev-lang/rust ~amd64\n*/* -~amd64",
                KeywordEvalResult::Rejected,
            ),
        ];

        let pkg = package(["~amd64"]);
        for (package_keywords, expected) in test_cases {
            let keywords = policy("", package_keywords);
            assert_eq!(keywords.eval(&pkg), expected);
        }
    }

    #[test]
    fn test_policy_precedence() -> anyhow::Result<()> {
        let profile = PackageAcceptKeywords::from_lines(
            "dev-lang/rust amd64".lines(),
            Precedence::Profile(0),
        )?;
        let user = PackageAcceptKeywords::from_lines("*/* -amd64".lines(), Precedence::User)?;
        let package_accept_keywords = user.inherit(&profile)?;
        let policy = EffectiveKeywords::new(Vec::default(), package_accept_keywords);

        let pkg = package(["amd64"]);
        assert_eq!(policy.eval(&pkg), KeywordEvalResult::Rejected);
        Ok(())
    }

    #[test]
    fn test_policy_installed_package() {
        let policy = policy("amd64", "");
        let pkg = InstalledPackage::new_unchecked(
            cpv("dev-lang/rust-1.0"),
            "gentoo".parse().unwrap(),
            pkg_metadata(&[("KEYWORDS", "amd64")]),
            EffectiveUse::default(),
        );
        assert_eq!(
            policy.eval(&pkg),
            KeywordEvalResult::Accepted {
                stable_in_use: true
            }
        );
    }

    #[test]
    fn test_policy_stable_keywords() {
        let keywords = policy("amd64 ~amd64", "");
        let pkg = package(["amd64"]);
        assert_eq!(
            keywords.eval(&pkg),
            KeywordEvalResult::Accepted {
                stable_in_use: false
            }
        );

        let keywords = policy("amd64 ~arm64", "");
        let pkg = package(["amd64", "arm64"]);
        assert_eq!(
            keywords.eval(&pkg),
            KeywordEvalResult::Accepted {
                stable_in_use: false
            }
        );

        let keywords = policy("amd64", "dev-lang/rust ~amd64");
        let pkg = package(["amd64"]);
        assert_eq!(
            keywords.eval(&pkg),
            KeywordEvalResult::Accepted {
                stable_in_use: false
            }
        );
    }
}
