mod number;
mod revision;
mod suffix;

use crate::deps::atom::{Atom, AtomOperator, AtomVariant};
use crate::package::version::suffix::VersionSuffixes;
use crate::regex::V_REV;
use anyhow::{Result, anyhow};
use number::VersionNumber;
use regex::Regex;
use revision::PackageRevision;
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::sync::LazyLock;

/// Regex to validate and parse `version`, `suffixes` and the `revision`.
static VERSION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"^{V_REV}$")).unwrap());

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
    pub fn new(version: &str, suffixes: Option<&str>, revision: Option<&str>) -> Result<Self> {
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
        format!("r{}", self.revision.source().unwrap_or("0"))
    }

    /// Returns the source-sensitive package version and revision.
    pub fn pvr(&self) -> String {
        let mut pvr = self.pv();
        if let Some(revision) = self.revision.source() {
            pvr.push_str(&format!("-r{revision}"));
        }
        pvr
    }

    /// Checks if the given `atom` matches this version.
    pub fn matches_atom(&self, atom: &Atom) -> bool {
        let Some(atom_ver) = &atom.version else {
            // If the atom doesn't specify a version, it matches any version
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
            AtomVariant::VersionWildcard => {
                let mut atom_iter = atom_ver.number.iter();
                let mut self_iter = self.number.iter();
                loop {
                    match (atom_iter.next(), self_iter.next()) {
                        (Some(a), Some(b)) if a == b => {}
                        // If the next component in the atom is None,
                        // it means the wildcard matches any remaining components
                        (None, _) => return true,
                        _ => return false,
                    }
                }
            }
        }
    }
}

impl TryFrom<&str> for PackageVersion {
    type Error = anyhow::Error;

    /// Create a `PackageVersion` from a full version string, for example: `1.2.3_alpha1-r1`.
    fn try_from(version: &str) -> Result<Self, Self::Error> {
        let caps = VERSION_RE
            .captures(version)
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
        if let Some(revision) = self.revision.source() {
            write!(f, "-r{revision}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_package_version_new() {
        let version = PackageVersion::new("2.0.1", Some("_beta2_p20240101"), Some("1")).unwrap();

        assert_eq!(version.to_string(), "2.0.1_beta2_p20240101-r1");
    }

    #[test]
    fn test_package_version_try_from() {
        for input in [
            "1.0.0",
            "1.0.0-r0",
            "1.2.3a_alpha1",
            "2.0.1_beta2_p20240101-r1",
        ] {
            let version = PackageVersion::try_from(input).unwrap();
            assert_eq!(version.to_string(), input);
        }
    }

    #[test]
    fn test_package_version_try_from_err() {
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
            "1.0.0-r-1",
        ] {
            assert!(PackageVersion::try_from(input).is_err());
        }
    }

    #[test]
    fn test_package_version_revision_source() {
        let padded = PackageVersion::new("1.0.0", None, Some("0302")).unwrap();
        let canonical = PackageVersion::new("1.0.0", None, Some("302")).unwrap();
        let mut versions = HashSet::new();

        versions.insert(padded.clone());
        versions.insert(canonical.clone());

        assert_eq!(padded, canonical);
        assert_eq!(padded.pr(), "r0302");
        assert_eq!(padded.pvr(), "1.0.0-r0302");
        assert_eq!(versions.len(), 1);

        let implicit = PackageVersion::new("1.0.0", None, None).unwrap();
        let explicit = PackageVersion::new("1.0.0", None, Some("0")).unwrap();

        assert_eq!(implicit.pr(), "r0");
        assert_eq!(explicit.pr(), "r0");
        assert_eq!(implicit.pvr(), "1.0.0");
        assert_eq!(explicit.pvr(), "1.0.0-r0");
    }

    #[test]
    fn test_package_version_explicit_zero_revision_matches_atom() {
        let version = PackageVersion::new("1.0.0", None, Some("0")).unwrap();
        let implicit_atom = Atom::new("=dev-libs/pkg-1.0.0").unwrap();
        let explicit_atom = Atom::new("=dev-libs/pkg-1.0.0-r0").unwrap();

        assert!(version.matches_atom(&implicit_atom));
        assert!(version.matches_atom(&explicit_atom));
    }
}
