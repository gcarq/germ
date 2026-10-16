use std::borrow::Borrow;
use std::fmt;
use std::str::FromStr;

use anyhow::{anyhow, bail};
use fluent_uri::Uri as FluentUri;
use rkyv::{Archive, Deserialize, Serialize};

use crate::deps::ExprItem;

/// Holds a validated URI.
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Debug)]
pub struct Uri(Box<str>);

impl Uri {
    /// Creates a new [`Uri`] from the given `value`.
    ///
    /// The URI must be absolute and specify a scheme.
    pub fn new(value: impl Into<Box<str>>) -> anyhow::Result<Self> {
        let value = value.into();
        let parsed = FluentUri::parse(value.as_ref())
            .map_err(|err| anyhow!("invalid URI '{value}': {err}"))?;

        let host = parsed.authority().map_or("", |a| a.host());
        if host.is_empty() {
            bail!("URI '{value}' is missing a host");
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for Uri {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for Uri {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for Uri {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl ExprItem for Uri {}

impl fmt::Display for Uri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uri_ok() {
        let uris = [
            "https://example.org",
            "http://example.org/path",
            "mirror://gentoo/distfiles/example.tar.gz",
            "https://example.org/a%20b",
        ];
        for uri in uris {
            let parsed = Uri::new(uri).unwrap();
            assert_eq!(parsed.as_str(), uri);
        }
    }

    #[test]
    fn test_uri_err() {
        let uris = [
            "example.org",
            "/file",
            "https://",
            "mailto:user@example.com",
        ];
        for uri in uris {
            assert!(Uri::new(uri).is_err(), "{uri:?} should be invalid");
        }
    }
}
