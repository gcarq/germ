use std::{mem, path::Path, str::FromStr};

use super::{AtomPolicies, AtomPolicy};
use crate::deps::atom::Atom;
use crate::files::entry::{Entry, EntryValue, Precedence};
use crate::repository::Arch;
use crate::utils::Inherit;
use anyhow::Context;

/// Holds all keyword selectors for packages.
#[derive(Clone, Default, Debug)]
pub struct PackageAcceptKeywords(AtomPolicies<KeywordSelectors>);

impl PackageAcceptKeywords {
    pub fn from_path(path: &Path, order: Precedence, recursive: bool) -> anyhow::Result<Self> {
        Ok(Self(AtomPolicies::from_path(path, order, recursive)?))
    }

    pub fn from_string(content: String, order: Precedence) -> anyhow::Result<Self> {
        Ok(Self(AtomPolicies::from_string(content, order)?))
    }

    pub fn get(&self, atom: &Atom) -> Option<&KeywordSelectors> {
        self.0.get(atom)
    }
}

impl Inherit for PackageAcceptKeywords {
    fn inherit_from(&mut self, parent: &Self) -> anyhow::Result<()> {
        self.0.inherit_from(&parent.0)
    }
}

/// This holds a collection of [`KeywordRule`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeywordSelectors(Vec<KeywordRule>);

impl KeywordSelectors {
    pub fn as_slice(&self) -> &[KeywordRule] {
        &self.0
    }

    fn contains_reset(&self) -> bool {
        self.0.iter().any(|r| matches!(r, KeywordRule::Reset(_)))
    }
}

impl AtomPolicy for KeywordSelectors {
    /// Parses a keyword policy for one package.
    fn parse(value: &str, precedence: Precedence) -> anyhow::Result<Self> {
        let mut rules = Vec::new();

        for keyword in value.split_ascii_whitespace() {
            if keyword == "-*" {
                rules.clear();
                rules.push(KeywordRule::Reset(precedence));
                continue;
            }

            let entry = Entry::from_str(keyword, precedence)
                .with_context(|| format!("invalid keyword: {keyword}"))?;
            rules.push(KeywordRule::Selector(entry));
        }

        // Default to testing if no selectors are specified.
        if rules.is_empty() {
            rules.push(KeywordRule::Selector(Entry::from_str("~*", precedence)?));
        }

        Ok(Self(rules))
    }

    /// Updates `self` with the given [`KeywordSelectors`],
    /// replacing existing keywords and handling resets.
    fn update_from(&mut self, other: Self) {
        if other.contains_reset() {
            self.0.clear();
        }
        self.0.extend(other.0);
    }
}

impl Inherit for KeywordSelectors {
    fn inherit_from(&mut self, parent: &Self) -> anyhow::Result<()> {
        if self.contains_reset() {
            return Ok(());
        }

        self.0 = parent
            .0
            .iter()
            .cloned()
            .chain(mem::take(&mut self.0))
            .collect();
        Ok(())
    }
}

impl EntryValue for KeywordSelector {}

/// A rule that can either be a keyword selector or a reset command,
/// e.g.: `amd64`, `~arm64`, `-*`, etc..
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeywordRule {
    Selector(Entry<KeywordSelector>),
    Reset(Precedence),
}

/// Represents a selector for keywords, which can be used to filter
/// packages based on their keyword status.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeywordSelector {
    Stable(Arch),  // arch
    Testing(Arch), // ~arch
    AnyStable,     // *
    AnyTesting,    // ~*
    Any,           // **
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_selectors() -> anyhow::Result<()> {
        let selectors = KeywordSelectors::parse(
            "amd64 ~arm64 * ~* ** -amd64 -~arm64 -~* -**",
            Precedence::User,
        )?;

        assert_eq!(
            selectors.as_slice(),
            &[
                KeywordRule::Selector(Entry::from_str("amd64", Precedence::User)?),
                KeywordRule::Selector(Entry::from_str("~arm64", Precedence::User)?),
                KeywordRule::Selector(Entry::from_str("*", Precedence::User)?),
                KeywordRule::Selector(Entry::from_str("~*", Precedence::User)?),
                KeywordRule::Selector(Entry::from_str("**", Precedence::User)?),
                KeywordRule::Selector(Entry::from_str("-amd64", Precedence::User)?),
                KeywordRule::Selector(Entry::from_str("-~arm64", Precedence::User)?),
                KeywordRule::Selector(Entry::from_str("-~*", Precedence::User)?),
                KeywordRule::Selector(Entry::from_str("-**", Precedence::User)?),
            ]
        );

        let selectors = KeywordSelectors::parse("amd64 -* ~arm64", Precedence::User)?;
        assert_eq!(
            selectors.as_slice(),
            &[
                KeywordRule::Reset(Precedence::User),
                KeywordRule::Selector(Entry::from_str("~arm64", Precedence::User)?),
            ]
        );
        Ok(())
    }

    #[test]
    fn test_parse_atom_only() -> anyhow::Result<()> {
        let entries =
            PackageAcceptKeywords::from_string("net-analyzer/netcat".into(), Precedence::User)?;
        let selectors = entries.get(&Atom::new("net-analyzer/netcat")?).unwrap();

        assert_eq!(
            selectors.as_slice(),
            vec![KeywordRule::Selector(Entry::from_str(
                "~*",
                Precedence::User
            )?)]
        );
        Ok(())
    }

    #[test]
    fn test_update_applies_reset() -> anyhow::Result<()> {
        let entries = PackageAcceptKeywords::from_string(
            r#"
                dev-lang/rust amd64
                dev-lang/rust -*
                dev-lang/rust ~amd64
            "#
            .into(),
            Precedence::User,
        )?;
        let selectors = entries.get(&Atom::new("dev-lang/rust")?).unwrap();

        assert_eq!(
            selectors.as_slice(),
            vec![
                KeywordRule::Reset(Precedence::User),
                KeywordRule::Selector(Entry::from_str("~amd64", Precedence::User)?),
            ]
        );
        Ok(())
    }

    #[test]
    fn test_inherit_parent_before_child() -> anyhow::Result<()> {
        let parent = PackageAcceptKeywords::from_string(
            "dev-lang/rust amd64".into(),
            Precedence::Profile(0),
        )?;
        let mut child = PackageAcceptKeywords::from_string(
            "dev-lang/rust -* ~amd64".into(),
            Precedence::Profile(1),
        )?;

        child.inherit_from(&parent)?;
        let selectors = child.get(&Atom::new("dev-lang/rust")?).unwrap();

        assert_eq!(
            selectors.as_slice(),
            &[
                KeywordRule::Reset(Precedence::Profile(1)),
                KeywordRule::Selector(Entry::from_str("~amd64", Precedence::Profile(1))?),
            ]
        );
        Ok(())
    }
}
