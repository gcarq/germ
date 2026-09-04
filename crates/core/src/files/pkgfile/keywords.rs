use std::{mem, path::Path, str::FromStr};

use anyhow::Context;

use crate::deps::atom::Atom;
use crate::files::content_from_path;
use crate::files::entry::{Entry, EntryValue, Precedence};
use crate::utils::{Inherit, strip_line_comment};
use crate::{repository::Arch, types::FxHashMap};

#[derive(Clone, Default, Debug)]
pub struct PackageAcceptKeywords(FxHashMap<Atom, KeywordSelectors>);

impl PackageAcceptKeywords {
    pub fn from_path(path: &Path, order: Precedence, recursive: bool) -> anyhow::Result<Self> {
        let content = content_from_path(path, recursive, true)?;
        Self::from_string(content, order)
            .with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn from_string(content: String, order: Precedence) -> anyhow::Result<Self> {
        let mut map = FxHashMap::default();

        for (lineno, entry) in content.lines().enumerate() {
            let entry = strip_line_comment(entry);
            if entry.is_empty() {
                continue;
            }
            let (atom, keywords) = Self::parse_line(entry, order)
                .with_context(|| format!("error in line {}: {entry}", lineno + 1))?;

            let entry: &mut KeywordSelectors = map.entry(atom).or_default();
            entry.update_from(keywords);
        }
        Ok(Self(map))
    }

    pub fn get(&self, atom: &Atom) -> Option<&KeywordSelectors> {
        self.0.get(atom)
    }

    fn parse_line(line: &str, order: Precedence) -> anyhow::Result<(Atom, KeywordSelectors)> {
        let (atom, keywords) = match line.split_once(char::is_whitespace) {
            Some((atom, keywords)) => (atom, keywords),
            None => (line, ""),
        };
        Ok((atom.parse()?, KeywordSelectors::from_str(keywords, order)?))
    }
}

impl Inherit for PackageAcceptKeywords {
    fn inherit_from(&mut self, parent: &Self) -> anyhow::Result<()> {
        for (atom, parent_keywords) in &parent.0 {
            if let Some(keywords) = self.0.get_mut(atom) {
                keywords.inherit_from(parent_keywords)?;
            } else {
                self.0.insert(atom.clone(), parent_keywords.clone());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeywordSelectors(Vec<KeywordRule>);

impl KeywordSelectors {
    pub fn as_slice(&self) -> &[KeywordRule] {
        &self.0
    }

    pub fn from_str(keywords: &str, order: Precedence) -> anyhow::Result<Self> {
        let mut rules = Vec::new();

        for keyword in keywords.split_ascii_whitespace() {
            if keyword == "-*" {
                rules.clear();
                rules.push(KeywordRule::Reset(order));
                continue;
            }

            let entry = Entry::from_str(keyword, order)
                .with_context(|| format!("invalid keyword: {keyword}"))?;
            rules.push(KeywordRule::Selector(entry));
        }

        // Default to testing if no selectors are specified.
        if rules.is_empty() {
            rules.push(KeywordRule::Selector(Entry::from_str("~*", order)?));
        }

        Ok(Self(rules))
    }

    pub fn update_from(&mut self, other: Self) {
        if other.contains_reset() {
            self.0.clear();
        }
        self.0.extend(other.0);
    }

    fn contains_reset(&self) -> bool {
        self.0.iter().any(|r| matches!(r, KeywordRule::Reset(_)))
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
                Some(arch) => Ok(Self::Testing(Arch::new(arch)?)),
                None => Ok(Self::Stable(Arch::new(selector)?)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_selectors() -> anyhow::Result<()> {
        let selectors = KeywordSelectors::from_str(
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
        Ok(())
    }

    #[test]
    fn test_parse_reset() -> anyhow::Result<()> {
        assert_eq!(
            KeywordSelectors::from_str("amd64 -* ~arm64", Precedence::User)?.as_slice(),
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
        let selectors = entries.0.get(&Atom::new("net-analyzer/netcat")?).unwrap();

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
        let selectors = entries.0.get(&Atom::new("dev-lang/rust")?).unwrap();

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
        let selectors = child.0.get(&Atom::new("dev-lang/rust")?).unwrap();

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
