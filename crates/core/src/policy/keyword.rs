use crate::deps::atom::Atom;
use crate::files::entry::Operation;
use crate::files::pkgfile::{KeywordRule, PackageAcceptKeywords};
use crate::keyword::{Keyword, KeywordSelector};
use crate::package::PackageView;

/// Immutable runtime policy that determines whether a package is accepted based on its keywords.
pub struct KeywordPolicy {
    accept_keywords: Vec<KeywordSelector>,
    rules: Vec<(Atom, KeywordAction)>,
}

impl KeywordPolicy {
    /// Builds [`KeywordPolicy`] from keyword records.
    pub fn new(
        accept_keywords: Option<&str>,
        package_accept_keywords: PackageAcceptKeywords,
    ) -> anyhow::Result<Self> {
        let accept_keywords = accept_keywords
            .map(|value| value.split_whitespace().map(str::parse).collect())
            .transpose()?
            .unwrap_or_default();

        let mut rules = package_accept_keywords
            .into_rules()
            .map(|(atom, rule)| (rule.precedence(), atom, KeywordAction::from(rule)))
            .collect::<Vec<_>>();
        rules.sort_by_key(|(precedence, _, _)| *precedence);

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

/// Defines a compiled keyword rule that can be used to check whether a keyword is accepted.
enum KeywordAction {
    Reset,
    Accept(KeywordSelector),
    Reject(KeywordSelector),
}

impl KeywordAction {
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
            Some(accept_keywords),
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
                    .map(|keyword| keyword.parse())
                    .collect::<anyhow::Result<_>>()
                    .unwrap(),
                ..Default::default()
            },
        )
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
    fn test_policy_global_keyword_invalid() {
        assert!(KeywordPolicy::new(Some("-amd64"), PackageAcceptKeywords::default()).is_err());
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
        let package_accept_keywords = user.inherit(&profile)?;
        let policy = KeywordPolicy::new(None, package_accept_keywords)?;

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
