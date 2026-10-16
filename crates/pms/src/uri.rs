use std::borrow::Borrow;
use std::fmt;
use std::str::FromStr;

use anyhow::anyhow;
use rkyv::{Archive, Deserialize, Serialize};

use crate::deps::ExprItem;

/// Holds a validated URI.
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Debug)]
pub struct Uri(Box<str>);

impl Uri {
    /// Creates a new [`Uri`] from the given `value`.
    ///
    /// The given `value` must be an absolute `proto://host/path` URI. See PMS 8.2.
    pub fn new(value: impl Into<Box<str>>) -> anyhow::Result<Self> {
        let value = value.into();
        parse_uri(&value).ok_or_else(|| anyhow!("invalid URI '{value}'"))?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns `true` if the scheme carries a `fetch+` or `mirror+` selective restriction.
    /// See PMS section 7.3.2.
    pub fn has_selective_restriction(&self) -> bool {
        parse_uri(&self.0).is_some_and(|(scheme, _)| {
            ["fetch+", "mirror+"].iter().any(|restriction| {
                scheme
                    .get(..restriction.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(restriction))
            })
        })
    }

    /// Returns the filename from the URI path, excluding any query or fragment.
    pub fn filename(&self) -> Option<&str> {
        let (_, path) = parse_uri(&self.0)?;
        path.rsplit('/').next().filter(|name| !name.is_empty())
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

/// Splits `input` into its scheme and path.
///
/// Expects an absolute URI of the form `proto://host/path`.
/// The scheme is returned as-is, while the path has no leading parameters or fragment.
fn parse_uri(input: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = input.split_once("://")?;

    let mut scheme_bytes = scheme.bytes();
    let valid_scheme = scheme_bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && scheme_bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'));
    if !valid_scheme {
        return None;
    }

    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = rest[..end].rsplit('@').next().unwrap_or_default();
    if host.is_empty() || host.starts_with(':') {
        return None;
    }

    let path = rest[end..].strip_prefix('/').unwrap_or_default();
    Some((scheme, path.split(['?', '#']).next().unwrap_or_default()))
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
            "http://vdr.websitec.de/download/vdr-calc/vdr-calc-0[1].0.1-rc5.tgz",
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

    #[test]
    fn test_uri_selective_restriction() {
        let cases = [
            ("https://example.org/a", false),
            ("mirror://gentoo/a", false),
            ("fetch+https://example.org/a", true),
            ("mirror+https://example.org/a", true),
            ("FETCH+https://example.org/a", true),
            ("Mirror+https://example.org/a", true),
        ];
        for (uri, expected) in cases {
            let uri = Uri::new(uri).unwrap();
            assert_eq!(uri.has_selective_restriction(), expected, "{uri}");
        }
    }

    #[test]
    fn test_uri_filename() {
        let cases = [
            ("https://example.org/a/b.tar.gz", Some("b.tar.gz")),
            ("https://example.org/a/b.tar.gz?x=1#frag", Some("b.tar.gz")),
            ("https://user@example.org:8080/a", Some("a")),
            ("mirror://gentoo/distfiles/a.tar.bz2", Some("a.tar.bz2")),
            ("http://example.org/a[1].tgz", Some("a[1].tgz")),
            ("https://example.org/a/b/", None),
            ("https://example.org/", None),
            ("https://example.org", None),
        ];
        for (uri, expected) in cases {
            let uri = Uri::new(uri).unwrap();
            assert_eq!(uri.filename(), expected, "{uri}");
        }
    }
}
