use std::borrow::Borrow;
use std::fmt;
use std::str::FromStr;

use anyhow::bail;
use rkyv::{Archive, Deserialize, Serialize};

use crate::atom::{Atom, BlockerStrength};
use crate::uri::Uri;
use crate::useflag::UseFlag;

/// The `SRC_URI` arrow that renames a source URI to a distfile.
pub const ARROW: &str = "->";

/// This trait defines an item that can be used in a dependency expression,
/// such as [`RequiredUseFlag`] and [`DepAtom`].
pub trait ExprItem: FromStr<Err = anyhow::Error> + fmt::Display {
    /// Whether items of this type accept the `SRC_URI` arrow syntax `left -> right`.
    const SUPPORTS_ARROW: bool = false;

    /// Validates if the item is valid in a dependency expression.
    ///
    /// Returns `Err` if the item is invalid.
    fn validate(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Extends an [`Atom`] with an optional blocker, e.g.:
/// `!<app-misc/foo-1.3` or `!!app-misc/foo`.
///
/// [`BlockerStrength`] only makes sense when used a dependency, and is not part of the atom itself.
#[derive(Archive, Serialize, Deserialize, Default, Clone, PartialEq, Eq, Hash, Debug)]
pub struct DepAtom {
    atom: Atom,
    blocker: Option<BlockerStrength>,
}

impl DepAtom {
    /// Returns the inner [`Atom`].
    pub const fn inner(&self) -> &Atom {
        &self.atom
    }

    /// Returns the blocker, if any.
    pub const fn blocker(&self) -> Option<BlockerStrength> {
        self.blocker
    }
}

impl ExprItem for DepAtom {
    fn validate(&self) -> anyhow::Result<()> {
        if self.atom.category().is_none()
            || self.atom.package().is_none()
            || self.atom.repo().is_some()
        {
            bail!("{self} is not valid in dependency expression");
        }
        Ok(())
    }
}

impl FromStr for DepAtom {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> anyhow::Result<Self> {
        let (blocker, atom) = match value.strip_prefix('!') {
            Some(atom) => match atom.strip_prefix("!") {
                Some(atom) => (Some(BlockerStrength::Strong), atom),
                None => (Some(BlockerStrength::Weak), atom),
            },
            None => (None, value),
        };
        let atom = Atom::new(atom)?;
        Ok(Self { blocker, atom })
    }
}

impl fmt::Display for DepAtom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(blocker) = &self.blocker {
            write!(f, "{blocker}")?;
        }
        write!(f, "{}", self.atom)
    }
}

/// Represents a required USE flag in a dependency expression.
///
/// Additional to the [`UseFlag`] it has the ability to be disabled.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub struct RequiredUseFlag {
    flag: UseFlag,
    negated: bool,
}

impl RequiredUseFlag {
    /// Returns the inner [`UseFlag`].
    pub const fn inner(&self) -> &UseFlag {
        &self.flag
    }

    /// Returns `true` if this required USE flag is negated.
    pub const fn is_negated(&self) -> bool {
        self.negated
    }
}

impl ExprItem for RequiredUseFlag {}

impl FromStr for RequiredUseFlag {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> anyhow::Result<Self> {
        let (flag, negated) = match value.strip_prefix('!') {
            Some(flag) => (flag, true),
            None => (value, false),
        };
        let flag = UseFlag::new(flag)?;
        Ok(Self { flag, negated })
    }
}

impl fmt::Display for RequiredUseFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.negated {
            f.write_str("!")?;
        }
        self.flag.fmt(f)
    }
}

/// A filename used to store `SRC_URI` items in `DISTDIR`.
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Debug)]
pub struct DistfileName(Box<str>);

impl DistfileName {
    /// Creates a new [`DistfileName`] from the given `name`.
    pub fn new(name: impl Into<Box<str>>) -> anyhow::Result<Self> {
        let name = name.into();
        if name.is_empty() || name.contains(['/', '\0']) || matches!(&*name, "." | "..") {
            bail!("invalid distfile name '{name}'");
        }
        Ok(Self(name))
    }

    /// Returns the distfile name as str.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for DistfileName {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for DistfileName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for DistfileName {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl fmt::Display for DistfileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A single source item of a `SRC_URI` expression.
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Debug)]
pub enum SrcUriItem {
    /// A source URI, optionally renamed to a distfile with `->`.
    Uri(Uri, Option<DistfileName>),
    File(DistfileName),
}

impl SrcUriItem {
    /// Returns the source URI, if present.
    pub const fn uri(&self) -> Option<&Uri> {
        match self {
            Self::Uri(uri, _) => Some(uri),
            Self::File(_) => None,
        }
    }

    /// Returns the distfile name, if present.
    pub fn distfile(&self) -> Option<&str> {
        match self {
            Self::Uri(_, Some(name)) | Self::File(name) => Some(name.as_str()),
            Self::Uri(uri, None) => uri.filename(),
        }
    }
}

impl ExprItem for SrcUriItem {
    const SUPPORTS_ARROW: bool = true;
}

impl FromStr for SrcUriItem {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = value.split_whitespace().collect();
        let item = match parts.as_slice() {
            [uri, ARROW, filename] if *filename != ARROW => {
                Self::Uri(Uri::new(*uri)?, Some(DistfileName::new(*filename)?))
            }
            [uri] if uri.contains("://") => Self::Uri(Uri::new(*uri)?, None),
            [filename] if *filename != ARROW => Self::File(DistfileName::new(*filename)?),
            _ => bail!("invalid source item '{value}'"),
        };
        Ok(item)
    }
}

impl fmt::Display for SrcUriItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uri(uri, Some(name)) => write!(f, "{uri} -> {name}"),
            Self::Uri(uri, None) => uri.fmt(f),
            Self::File(name) => name.fmt(f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distfile_name() {
        let valid = ["foo.tar.gz", "a-b.tar.xz"];
        for name in valid {
            assert_eq!(DistfileName::new(name).unwrap().as_str(), name);
        }
        let invalid = ["", "a/b", "..", "."];
        for name in invalid {
            assert!(DistfileName::new(name).is_err(), "{name}");
        }
    }

    #[test]
    fn test_src_uri_item() {
        let cases = [
            ("https://example.org/a", Some("a")),
            ("a.tar.gz", Some("a.tar.gz")),
            ("https://example.org/latest -> a.tar.gz", Some("a.tar.gz")),
            ("http://example.org/a[1].tgz", Some("a[1].tgz")),
        ];
        for (input, distfile) in cases {
            let item = input.parse::<SrcUriItem>().unwrap();
            assert_eq!(item.to_string(), input);
            assert_eq!(item.distfile(), distfile);
        }

        let uri = "https://example.org/a".parse::<SrcUriItem>().unwrap();
        assert_eq!(uri.uri().map(Uri::as_str), Some("https://example.org/a"));
        let file = "a.tar.gz".parse::<SrcUriItem>().unwrap();
        assert!(file.uri().is_none());
    }

    #[test]
    fn test_src_uri_item_err() {
        let cases = ["->", "a.tar.gz -> b.tar.gz", "https://example.org/a ->"];
        for input in cases {
            assert!(input.parse::<SrcUriItem>().is_err(), "{input}");
        }
    }
}
