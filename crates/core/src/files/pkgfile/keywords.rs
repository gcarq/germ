use std::mem;

use anyhow::Context;
use germ_pms::Atom;

use super::{AtomPolicies, AtomPolicy};
use crate::files::entry::{Entry, EntryValue, LineOrigin, Precedence};
use crate::keyword::KeywordSelector;
use crate::utils::Inherit;

/// Represents the content of a package accept keywords file.
///
/// This should not be used as a source of truth for keyword acceptance,
/// but rather as a representation of the package accept keywords file content.
///
/// It can only be used for keyword acceptance after inheriting all files
/// and compiling a keyword policy.
pub type PackageAcceptKeywords = AtomPolicies<KeywordSpec>;

impl AtomPolicies<KeywordSpec> {
    /// Consumes this configuration and returns [`Atom`] and [`KeywordRule`] pairs.
    pub fn into_rules(self) -> impl Iterator<Item = (Atom, KeywordRule)> {
        self.into_iter()
            .flat_map(|(atom, spec)| spec.0.into_iter().map(move |rule| (atom.clone(), rule)))
    }
}

/// Helper struct to manage keyword rules for a single atom,
/// while parsing `package.accept_keywords` files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeywordSpec(Vec<KeywordRule>);

impl KeywordSpec {
    fn contains_reset(&self) -> bool {
        self.0.iter().any(|r| matches!(r, KeywordRule::Reset(_)))
    }
}

impl AtomPolicy for KeywordSpec {
    /// Parses a keyword policy for one package.
    fn parse(value: &str, precedence: Precedence, origin: LineOrigin) -> anyhow::Result<Self> {
        let mut rules = Vec::new();

        for keyword in value.split_ascii_whitespace() {
            if keyword == "-*" {
                rules.clear();
                rules.push(KeywordRule::Reset(precedence));
                continue;
            }

            let entry = Entry::new(keyword, precedence, origin.clone())
                .with_context(|| format!("invalid keyword: {keyword}"))?;
            rules.push(KeywordRule::Selector(entry));
        }

        if rules.is_empty() {
            rules.push(KeywordRule::AtomOnly(precedence));
        }

        Ok(Self(rules))
    }

    /// Updates `self` with the given [`KeywordSpec`],
    /// replacing existing keywords and handling resets.
    fn update_from(&mut self, other: Self) {
        if other.contains_reset() {
            self.0.clear();
        }
        self.0.extend(other.0);
    }
}

impl Inherit for KeywordSpec {
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

/// Represents a keyword selector or reset command,
/// e.g.: `amd64`, `~arm64`, `-*`, etc..
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeywordRule {
    Selector(Entry<KeywordSelector>),
    /// An atom-only entry without keyword is equivalent to `testing`
    /// for all configured arches via `ACCEPT_KEYWORDS`.
    AtomOnly(Precedence),
    Reset(Precedence),
}

impl KeywordRule {
    /// Returns the source precedence.
    pub const fn precedence(&self) -> Precedence {
        match self {
            Self::Selector(entry) => entry.prec,
            Self::AtomOnly(prec) | Self::Reset(prec) => *prec,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::test_support::{entry, origin};

    #[test]
    fn test_keyword_spec_parse() -> anyhow::Result<()> {
        let spec = KeywordSpec::parse("amd64 -~arm64 **", Precedence::User, origin(1))?;

        assert_eq!(
            spec.0,
            [
                KeywordRule::Selector(entry("amd64", Precedence::User, 1)),
                KeywordRule::Selector(entry("-~arm64", Precedence::User, 1)),
                KeywordRule::Selector(entry("**", Precedence::User, 1)),
            ]
        );

        let spec = KeywordSpec::parse("amd64 -* ~arm64", Precedence::User, origin(1))?;
        assert_eq!(
            spec.0,
            [
                KeywordRule::Reset(Precedence::User),
                KeywordRule::Selector(entry("~arm64", Precedence::User, 1)),
            ]
        );
        Ok(())
    }

    #[test]
    fn test_atom_only() -> anyhow::Result<()> {
        let entries =
            PackageAcceptKeywords::from_lines("net-analyzer/netcat".lines(), Precedence::User)?;
        let rules = entries.into_rules().collect::<Vec<_>>();

        let atom = Atom::new("net-analyzer/netcat")?;
        assert_eq!(rules, [(atom, KeywordRule::AtomOnly(Precedence::User)),]);
        Ok(())
    }

    #[test]
    fn test_reset() -> anyhow::Result<()> {
        let entries = PackageAcceptKeywords::from_lines(
            [
                "dev-lang/rust amd64",
                "dev-lang/rust -*",
                "dev-lang/rust ~amd64",
            ],
            Precedence::User,
        )?;
        let rules = entries
            .into_rules()
            .map(|(_, rule)| rule)
            .collect::<Vec<_>>();

        assert_eq!(
            rules,
            [
                KeywordRule::Reset(Precedence::User),
                KeywordRule::Selector(entry("~amd64", Precedence::User, 3)),
            ]
        );

        let parent = PackageAcceptKeywords::from_lines(
            "dev-lang/rust amd64".lines(),
            Precedence::Profile(0),
        )?;
        let mut child = PackageAcceptKeywords::from_lines(
            "dev-lang/rust -* ~amd64".lines(),
            Precedence::Profile(1),
        )?;

        child.inherit_from(&parent)?;
        let rules = child.into_rules().map(|(_, rule)| rule).collect::<Vec<_>>();

        assert_eq!(
            rules,
            [
                KeywordRule::Reset(Precedence::Profile(1)),
                KeywordRule::Selector(entry("~amd64", Precedence::Profile(1), 1)),
            ]
        );
        Ok(())
    }
}
