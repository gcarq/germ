mod base;
mod matching;
mod numeric;
mod revision;
mod suffix;

use crate::deps::atom::{Atom, AtomOperator, AtomVariant};
use crate::grammar::{REVISION, VERSION, VERSION_SUFFIXES};
use anyhow::anyhow;
use base::VersionNumber;
use fancy_regex::Regex;
use revision::PackageRevision;
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::fmt::Write;
use std::sync::LazyLock;
use suffix::VersionSuffixes;

/// Regex to validate and parse `version`, `suffixes` and the `revision`.
static VERSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"\A(?<version>{VERSION})(?<suffixes>{VERSION_SUFFIXES})(?:-r(?<revision>{REVISION}))?\z"
    ))
    .unwrap()
});

/// Represents a package version according to PMS section 3.2 and 3.3.
///
/// This includes the base version components (e.g., `1.2.3a`), any suffixes
/// (e.g., `_alpha1`, `_p20240101`), and the revision number (e.g., `-r1`).
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct PackageVersion {
    number: VersionNumber,
    suffixes: VersionSuffixes,
    pub revision: PackageRevision,
}

impl PackageVersion {
    /// Creates a new [`PackageVersion`] from the given `version`, `suffixes`, and `revision`.
    ///
    /// For example: `PackageVersion::new("1.2.3a", Some("_alpha1_p20240101"), "1")`
    pub fn new(
        version: &str,
        suffixes: Option<&str>,
        revision: Option<&str>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            number: version.parse()?,
            suffixes: match suffixes {
                Some(s) => s.parse()?,
                None => VersionSuffixes::default(),
            },
            revision: PackageRevision::new(revision)?,
        })
    }

    /// Returns the version, with no revision. For example `7.0.174`.
    pub fn pv(&self) -> String {
        format!("{}{}", self.number, self.suffixes)
    }

    /// Returns the revision, or `r0` if none exists.
    pub fn pr(&self) -> String {
        format!("r{}", self.revision.as_str().unwrap_or("0"))
    }

    /// Returns the source-sensitive package version and revision.
    pub fn pvr(&self) -> String {
        let mut pvr = self.pv();
        if let Some(revision) = self.revision.as_str() {
            write!(pvr, "-r{revision}").unwrap();
        }
        pvr
    }

    /// Checks if the given `atom` matches this version.
    pub fn matches_atom(&self, atom: &Atom) -> bool {
        let Some(atom_ver) = &atom.version else {
            return true;
        };

        match atom.variant {
            AtomVariant::Simple => self == atom_ver,
            AtomVariant::VersionOperator => match atom.operator {
                Some(AtomOperator::Less) => self < atom_ver,
                Some(AtomOperator::LessEqual) => self <= atom_ver,
                Some(AtomOperator::Equal) => self == atom_ver,
                Some(AtomOperator::Greater) => self > atom_ver,
                Some(AtomOperator::GreaterEqual) => self >= atom_ver,
                Some(AtomOperator::Approximate) => {
                    self.number == atom_ver.number && self.suffixes == atom_ver.suffixes
                }
                None => unreachable!("BUG: atom is expected to have an operator"),
            },
            AtomVariant::VersionWildcard => matching::matches_wildcard(atom_ver, self),
        }
    }
}

impl TryFrom<&str> for PackageVersion {
    type Error = anyhow::Error;

    /// Create a `PackageVersion` from a full version string, for example: `1.2.3_alpha1-r1`.
    fn try_from(version: &str) -> anyhow::Result<Self> {
        let caps = VERSION_RE
            .captures(version)?
            .ok_or_else(|| anyhow!("invalid version: '{version}'"))?;
        Self::new(
            &caps["version"],
            Some(&caps["suffixes"]),
            caps.name("revision").map(|m| m.as_str()),
        )
    }
}

impl fmt::Display for PackageVersion {
    /// Returns the source-sensitive version string.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.number, self.suffixes)?;
        if let Some(revision) = self.revision.as_str() {
            write!(f, "-r{revision}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::assert_eq_hash;
    use std::cmp::Ordering;

    #[test]
    fn test_package_version_new() {
        let version = PackageVersion::new("2.0.1", Some("_beta2_p20240101"), Some("1")).unwrap();
        assert_eq!(version.to_string(), "2.0.1_beta2_p20240101-r1");
    }

    #[test]
    fn test_package_version_ordering() {
        let cases = [
            ("1.2.03", "1.2.3", Ordering::Less),
            ("1.2.3", "1.2.4", Ordering::Less),
            ("1.2.3", "1.2.3a", Ordering::Less),
            ("1.2.3a", "1.2.3b", Ordering::Less),
            ("1.2.3b", "1.2.3.1", Ordering::Less),
            ("1.2.3.1", "1.2.4", Ordering::Less),
            ("1.2.4", "1.10.0", Ordering::Less),
            ("1.10.0", "1.10.0.1", Ordering::Less),
            ("1.10.0.1", "2.0", Ordering::Less),
            (
                "999999999999999999999999999999999",
                "1999999999999999999999999999999999",
                Ordering::Less,
            ),
            ("2.0", "20251122", Ordering::Less),
            ("1_alpha", "1_beta_p", Ordering::Less),
            ("1_beta", "1_alpha_p", Ordering::Greater),
            ("1_alpha_beta_p", "1_alpha", Ordering::Less),
            ("1_alpha", "1_alpha_p", Ordering::Less),
            ("1_alpha", "1_alpha0", Ordering::Equal),
            ("1", "1-r0", Ordering::Equal),
            ("1-r1", "1-r2", Ordering::Less),
        ];

        for (left, right, expected) in cases {
            let left = PackageVersion::try_from(left).unwrap();
            let right = PackageVersion::try_from(right).unwrap();
            assert_eq!(left.cmp(&right), expected);
        }
    }

    #[test]
    fn test_package_version_hash() {
        for (left, right) in [
            ("1", "01"),
            ("1.030", "1.03"),
            ("1_alpha01", "1_alpha1"),
            ("1_alpha", "1_alpha0"),
            ("1", "1-r0"),
            ("1-r03", "1-r3"),
        ] {
            let left = PackageVersion::try_from(left).unwrap();
            let right = PackageVersion::try_from(right).unwrap();

            assert_eq_hash(&left, &right);
        }
    }

    #[test]
    fn test_package_version_parse() {
        for input in [
            "1",
            "1-r2",
            "1_alpha",
            "1_alpha0",
            "1.0.0",
            "1.0.0-r0",
            "1.2.3a_alpha1",
            "2.0.1_beta2_p20240101-r1",
            "1_alpha999999999999999999999999",
            "1-r999999999999999999999999999999999",
            "1_alpha999999999999999999999999-r999999999999999999999999999999999",
        ] {
            let version = PackageVersion::try_from(input).unwrap();
            assert_eq!(version.to_string(), input);
        }
    }

    #[test]
    fn test_package_version_parse_error() {
        for input in [
            "",
            "1.1.1aa",
            "0.33.1A",
            "1..0",
            "a.b.c",
            "20251212_ALPHA-r9999",
            "1.2.3a_unknownsuffix",
            "2.0.1_betaX-r1",
            "1.0.0-ra",
            "1.0.0-r",
            "1.0.0-r-1",
            "1.0.0:-r1",
        ] {
            assert!(PackageVersion::try_from(input).is_err());
        }
    }

    #[test]
    fn test_package_version_revision_spelling() {
        let padded = PackageVersion::new("1.0.0", None, Some("0302")).unwrap();
        let canonical = PackageVersion::new("1.0.0", None, Some("302")).unwrap();

        assert_eq_hash(&padded, &canonical);
        assert_eq!(padded.pr(), "r0302");
        assert_eq!(padded.pvr(), "1.0.0-r0302");
    }

    #[test]
    fn test_package_version_zero_revision() {
        let implicit = PackageVersion::new("1.0.0", None, None).unwrap();
        let explicit = PackageVersion::new("1.0.0", None, Some("0")).unwrap();

        assert_eq_hash(&implicit, &explicit);
        assert_eq!(implicit.pr(), "r0");
        assert_eq!(explicit.pr(), "r0");
        assert_eq!(implicit.pvr(), "1.0.0");
        assert_eq!(explicit.pvr(), "1.0.0-r0");
    }

    #[test]
    fn test_package_version_wildcard_atom() {
        let version = PackageVersion::new("15.2.1a", None, None).unwrap();
        let cases = [
            ("=dev-libs/pkg-15*", true),
            ("=dev-libs/pkg-15.2*", true),
            ("=dev-libs/pkg-15.2.1*", true),
            ("=dev-libs/pkg-15.2.1a*", true),
            ("=dev-libs/pkg-15.2.1b*", false),
            ("=dev-libs/pkg-15.2.2*", false),
        ];

        for (atom, expected) in cases {
            assert_eq!(version.matches_atom(&Atom::new(atom).unwrap()), expected);
        }
    }

    #[test]
    fn test_package_version_zero_revision_atom() {
        let version = PackageVersion::new("1.0.0", None, Some("0")).unwrap();
        for atom in ["=dev-libs/pkg-1.0.0", "=dev-libs/pkg-1.0.0-r0"] {
            assert!(version.matches_atom(&Atom::new(atom).unwrap()));
        }
    }
}
