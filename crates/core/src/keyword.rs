use std::{fmt, str::FromStr};

use rkyv::{Archive, Deserialize, Serialize};

use crate::deps::atom::Atom;
use crate::files::entry::Operation;
use crate::files::pkgfile::{KeywordRule, PackageAcceptKeywords};
use crate::makenv::MakeEnv;
use crate::package::PackageView;
use crate::repository::Arch;

/// Represents a configured keyword status of a package for a given architecture.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Keyword {
    Stable(Arch),        // arch
    Testing(Arch),       // ~arch
    Unsupported(Arch),   // -arch
    UnlistedUnsupported, // -*, indicates not worth trying to test on unlisted archs.
}

impl Keyword {
    /// Returns the associated arch, if any.
    pub const fn arch(&self) -> Option<&Arch> {
        match self {
            Keyword::Stable(arch) | Keyword::Testing(arch) | Keyword::Unsupported(arch) => {
                Some(arch)
            }
            Keyword::UnlistedUnsupported => None,
        }
    }
}

impl FromStr for Keyword {
    type Err = anyhow::Error;

    fn from_str(keyword: &str) -> Result<Self, Self::Err> {
        let keyword = match keyword.chars().next().unwrap_or_default() {
            '~' => Keyword::Testing(keyword[1..].parse()?),
            '-' => match keyword {
                "-*" => Keyword::UnlistedUnsupported,
                _ => Keyword::Unsupported(keyword[1..].parse()?),
            },
            _ => Keyword::Stable(keyword.parse()?),
        };
        Ok(keyword)
    }
}

impl fmt::Display for Keyword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Keyword::Stable(arch) => write!(f, "{arch}"),
            Keyword::Testing(arch) => write!(f, "~{arch}"),
            Keyword::Unsupported(arch) => write!(f, "-{arch}"),
            Keyword::UnlistedUnsupported => write!(f, "-*"),
        }
    }
}

/// Represents a configured keyword selector, used to match against package keywords.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeywordSelector {
    Stable(Arch),  // arch
    Testing(Arch), // ~arch
    AnyStable,     // *
    AnyTesting,    // ~*
    Any,           // **, also matches empty keywords
}

impl KeywordSelector {
    /// Checks if the given metadata keyword matches this selector.
    pub fn matches(&self, keyword: Option<&Keyword>) -> bool {
        match (self, keyword) {
            (Self::Stable(expected), Some(Keyword::Stable(actual))) => expected == actual,
            (Self::Testing(expected), Some(Keyword::Testing(actual))) => expected == actual,
            (Self::AnyStable, Some(Keyword::Stable(_))) => true,
            (Self::AnyTesting, Some(Keyword::Testing(_))) => true,
            (Self::Any, _) => true,
            _ => false,
        }
    }
}

impl FromStr for KeywordSelector {
    type Err = anyhow::Error;

    fn from_str(selector: &str) -> Result<Self, Self::Err> {
        match selector {
            "*" => Ok(Self::AnyStable),
            "~*" => Ok(Self::AnyTesting),
            "**" => Ok(Self::Any),
            _ => match selector.strip_prefix('~') {
                Some(arch) => Ok(Self::Testing(arch.parse()?)),
                None => Ok(Self::Stable(selector.parse()?)),
            },
        }
    }
}

/// Immutable runtime policy that determines whether
/// a package is accepted based on its keywords.
pub struct KeywordPolicy {
    accept_keywords: Vec<KeywordSelector>,
    rules: Vec<(Atom, KeywordAction)>,
}

impl KeywordPolicy {
    /// Builds a keyword policy from the given `makenv`
    /// and the user provided keyword configuration.
    pub fn new(makenv: &MakeEnv, keywords: PackageAcceptKeywords) -> anyhow::Result<Self> {
        let accept_keywords = makenv
            .get("ACCEPT_KEYWORDS")
            .map(|value| {
                value
                    .to_string()
                    .split_whitespace()
                    .map(KeywordSelector::from_str)
                    .collect()
            })
            .transpose()?
            .unwrap_or_default();

        let mut rules = keywords
            .into_rules()
            .map(|(atom, rule)| (rule.precedence(), atom, KeywordAction::from(rule)))
            .collect::<Vec<_>>();
        rules.sort_by_key(|(prec, _, _)| *prec);

        Ok(Self {
            accept_keywords,
            rules: rules
                .into_iter()
                .map(|(_, atom, action)| (atom, action))
                .collect(),
        })
    }

    /// Returns whether any metadata keyword for [`PackageView`] is accepted.
    pub fn evaluate<P: PackageView>(&self, package: &P) -> bool {
        if package.metadata().keywords.is_empty() {
            return self.evaluate_keyword(package, None);
        }

        package
            .metadata()
            .keywords
            .iter()
            .any(|keyword| self.evaluate_keyword(package, Some(keyword)))
    }

    /// Evaluates a single metadata keyword for [`PackageView`].
    fn evaluate_keyword<P: PackageView>(&self, package: &P, keyword: Option<&Keyword>) -> bool {
        let mut accepted = self
            .accept_keywords
            .iter()
            .any(|selector| selector.matches(keyword));

        for (atom, action) in &self.rules {
            if package.matches_atom(atom) {
                action.apply(keyword, &mut accepted);
            }
        }

        accepted
    }
}

/// Defines a compiled keyword rule that can be used to check
/// whether a keyword should be accepted or not.
enum KeywordAction {
    Reset,
    Accept(KeywordSelector),
    Reject(KeywordSelector),
}

impl KeywordAction {
    /// Checks whether the given `keyword` is accepted.
    ///
    /// Updates the `accepted` flag based on the action.
    fn apply(&self, keyword: Option<&Keyword>, accepted: &mut bool) {
        match self {
            Self::Reset => *accepted = false,
            Self::Accept(selector) if selector.matches(keyword) => *accepted = true,
            Self::Reject(selector) if selector.matches(keyword) => *accepted = false,
            _ => {}
        }
    }
}

impl From<KeywordRule> for KeywordAction {
    fn from(rule: KeywordRule) -> Self {
        match rule {
            KeywordRule::Reset(_) => Self::Reset,
            KeywordRule::Selector(entry) => match entry.op {
                Operation::Set => Self::Accept(entry.into_inner()),
                Operation::Unset => Self::Reject(entry.into_inner()),
            },
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

    fn policy(accept_keywords: &str, package_keywords: &str) -> anyhow::Result<KeywordPolicy> {
        KeywordPolicy::new(
            &MakeEnv::from_string(format!("ACCEPT_KEYWORDS=\"{accept_keywords}\""))?,
            PackageAcceptKeywords::from_string(package_keywords.into(), Precedence::User)?,
        )
    }

    fn package(keywords: &[&str]) -> Package {
        Package::new(
            cpv("dev-lang", "rust", "1.0"),
            "gentoo".parse().unwrap(),
            PackageMetadata {
                keywords: keywords
                    .iter()
                    .map(|keyword| Keyword::from_str(keyword))
                    .collect::<anyhow::Result<_>>()
                    .unwrap(),
                ..Default::default()
            },
        )
    }

    #[test]
    fn test_keyword_valid() {
        let tests = [
            ("amd64", Keyword::Stable("amd64".parse().unwrap())),
            ("~arm64", Keyword::Testing("arm64".parse().unwrap())),
            ("-x86", Keyword::Unsupported("x86".parse().unwrap())),
            ("-*", Keyword::UnlistedUnsupported),
        ];
        for (input, expected) in tests {
            assert_eq!(Keyword::from_str(input).unwrap(), expected);
        }
    }

    #[test]
    fn test_keyword_invalid() {
        for input in ["", "!", "~", "-", "~~amd64", "-*x86"] {
            assert!(Keyword::from_str(input).is_err());
        }
    }

    #[test]
    fn test_keyword_selector_match() -> anyhow::Result<()> {
        assert!(KeywordSelector::from_str("amd64")?.matches(Some(&Keyword::from_str("amd64")?)));
        assert!(KeywordSelector::from_str("~*")?.matches(Some(&Keyword::from_str("~arm64")?)));
        assert!(!KeywordSelector::from_str("*")?.matches(Some(&Keyword::from_str("~amd64")?)));
        assert!(!KeywordSelector::from_str("~*")?.matches(Some(&Keyword::from_str("-amd64")?)));
        assert!(KeywordSelector::from_str("**")?.matches(Some(&Keyword::UnlistedUnsupported)));
        assert!(KeywordSelector::from_str("**")?.matches(None));
        Ok(())
    }

    #[test]
    fn test_policy_global_keywords() -> anyhow::Result<()> {
        let policy = policy("amd64 ~arm64", "")?;
        assert!(policy.evaluate(&package(&["amd64"])));
        assert!(policy.evaluate(&package(&["~arm64"])));
        assert!(!policy.evaluate(&package(&["~amd64"])));
        Ok(())
    }

    #[test]
    fn test_policy_global_keyword_invalid() -> anyhow::Result<()> {
        let makenv = MakeEnv::from_string("ACCEPT_KEYWORDS=\"-amd64\"".into())?;
        assert!(KeywordPolicy::new(&makenv, PackageAcceptKeywords::default()).is_err());
        Ok(())
    }

    #[test]
    fn test_policy_global_wildcards() -> anyhow::Result<()> {
        let stable = policy("*", "")?;
        assert!(stable.evaluate(&package(&["amd64"])));
        assert!(!stable.evaluate(&package(&["~amd64"])));

        let testing = policy("~*", "")?;
        assert!(testing.evaluate(&package(&["~amd64"])));
        assert!(!testing.evaluate(&package(&["amd64"])));

        let any = policy("**", "")?;
        assert!(any.evaluate(&package(&[])));
        assert!(any.evaluate(&package(&["-amd64"])));
        Ok(())
    }

    #[test]
    fn test_policy_selector_reject() -> anyhow::Result<()> {
        let policy = policy("amd64", "dev-lang/rust -amd64")?;
        assert!(!policy.evaluate(&package(&["amd64"])));
        Ok(())
    }

    #[test]
    fn test_policy_reset_accept() -> anyhow::Result<()> {
        let policy = policy("amd64", "dev-lang/rust -* ~amd64")?;
        assert!(policy.evaluate(&package(&["~amd64"])));
        Ok(())
    }

    #[test]
    fn test_policy_atom_order() -> anyhow::Result<()> {
        let keywords = policy(
            "",
            "*/* -~amd64
            dev-lang/rust ~amd64",
        )?;
        assert!(keywords.evaluate(&package(&["~amd64"])));

        let keywords = policy(
            "",
            "dev-lang/rust ~amd64
            */* -~amd64",
        )?;
        assert!(!keywords.evaluate(&package(&["~amd64"])));

        Ok(())
    }

    #[test]
    fn test_policy_precedence() -> anyhow::Result<()> {
        let profile = PackageAcceptKeywords::from_string(
            "dev-lang/rust amd64".into(),
            Precedence::Profile(0),
        )?;
        let user = PackageAcceptKeywords::from_string("*/* -amd64".into(), Precedence::User)?;
        let merged = user.inherit(&profile)?;
        let policy = KeywordPolicy::new(&MakeEnv::default(), merged)?;

        assert!(!policy.evaluate(&package(&["amd64"])));
        Ok(())
    }

    #[test]
    fn test_policy_any() -> anyhow::Result<()> {
        let policy = policy("", "dev-lang/rust **")?;
        assert!(policy.evaluate(&package(&[])));
        assert!(policy.evaluate(&package(&["-amd64"])));
        assert!(policy.evaluate(&package(&["-*"])));
        Ok(())
    }

    #[test]
    fn test_policy_installed_package() -> anyhow::Result<()> {
        let policy = policy("amd64", "")?;
        let package = InstalledPackage {
            cpv: cpv("dev-lang", "rust", "1.0"),
            repo: "gentoo".parse().unwrap(),
            metadata: PackageMetadata {
                keywords: vec!["amd64".parse()?],
                ..Default::default()
            },
            use_flags: Vec::new(),
        };
        assert!(policy.evaluate(&package));
        Ok(())
    }
}
