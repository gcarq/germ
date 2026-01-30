use crate::package::version::suffix::VersionSuffixes;
use anyhow::{Context, Result, anyhow};
use number::VersionNumber;
use std::fmt;
use std::str::FromStr;

pub mod number;
pub mod suffix;

/// Represents a package version according to PMS section 3.2 and 3.3.
/// This includes the base version components (e.g., "1.2.3a"), any suffixes
/// (e.g., "_alpha1", "_p20240101"), and the revision number (e.g., "-r1").
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct PackageVersion {
    pub number: VersionNumber,
    pub suffixes: VersionSuffixes,
    pub revision: usize,
}

impl PackageVersion {
    /// Creates a new [`PackageVersion`] from the given `version`, `suffixes`, and `revision`.
    /// For example: `PackageVersion::new("1.2.3a", Some("_alpha1_p20240101"), "1")`
    pub fn new(version: &str, suffixes: Option<&str>, revision: Option<&str>) -> Result<Self> {
        Ok(Self {
            number: VersionNumber::from_str(version)?,
            suffixes: match suffixes {
                Some(s) => VersionSuffixes::from_str(s)?,
                None => VersionSuffixes::default(),
            },
            revision: match revision {
                Some(rev) => rev
                    .parse::<usize>()
                    .with_context(|| anyhow!("revision must be a positive integer, got '{rev}'"))?,
                None => 0,
            },
        })
    }

    /// Returns the version, with no revision. For example `7.0.174`.
    pub fn v(&self) -> String {
        format!("{}{}", self.number, self.suffixes)
    }

    /// Returns the revision, or `r0` if none exists.
    pub fn r(&self) -> String {
        match self.revision {
            0 => "r0".to_owned(),
            rev => format!("r{}", rev),
        }
    }

    /// Returns the version and revision (if any), for example `7.0.174` or `7.0.174-r1`.
    pub fn vr(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for PackageVersion {
    /// Returns the full version string including suffixes and revision,
    /// for example: `1.2.3_alpha1-r1`. This is also referred to as the `PVR` in PMS.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.number, self.suffixes)?;
        if self.revision > 0 {
            write!(f, "-r{}", self.revision)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_version_new_ok() {
        let test_cases = vec![
            PackageVersion::new("1.0.0", None, None),
            PackageVersion::new("1.2.3a", Some("_alpha1"), None),
            PackageVersion::new("2.0.1", Some("_beta2_p20240101"), Some("1")),
            PackageVersion::new("2.3.4z", Some("alpha_p20250101"), Some("2")),
            PackageVersion::new("9999", None, None),
        ];
        for version in test_cases {
            version.unwrap();
        }
    }

    #[test]
    fn test_package_version_new_err() {
        let test_cases = vec![
            PackageVersion::new("", None, None),
            PackageVersion::new("1.1.1aa", None, None),
            PackageVersion::new("0.33.1A", None, None),
            PackageVersion::new("1..0", None, None),
            PackageVersion::new("a.b.c", None, None),
            PackageVersion::new("20251212", Some("_ALPHA"), Some("9999")),
            PackageVersion::new("1.2.3a", Some("_unknownsuffix"), None),
            PackageVersion::new("2.0.1", Some("_betaX"), Some("1")),
            PackageVersion::new("1.0.0", None, Some("a")),
            PackageVersion::new("1.0.0", None, Some("-1")),
        ];
        for version in test_cases {
            assert!(version.is_err(), "Expected error for: {}", version.unwrap());
        }
    }

    #[test]
    fn test_package_version_display() {
        let test_cases = vec![
            (PackageVersion::new("1.0.0", None, None), "1.0.0"),
            (
                PackageVersion::new("1.2.3a", Some("_alpha1"), None),
                "1.2.3a_alpha1",
            ),
            (
                PackageVersion::new("2.0.1", Some("_beta2_p20240101"), Some("1")),
                "2.0.1_beta2_p20240101-r1",
            ),
            (
                PackageVersion::new("2.3.4z", Some("alpha_p20250101"), Some("2")),
                "2.3.4z_alpha_p20250101-r2",
            ),
        ];
        for (pkg_version, expected_str) in test_cases {
            assert_eq!(pkg_version.unwrap().to_string(), expected_str);
        }
    }

    #[test]
    fn test_package_version_ord_revision() {
        let v1_r0 = PackageVersion::new("1.0.0", None, None).unwrap();
        let v1_r1 = PackageVersion::new("1.0.0", None, Some("1")).unwrap();
        let v1_r2 = PackageVersion::new("1.0.0", None, Some("2")).unwrap();

        assert!(v1_r0 < v1_r1);
        assert!(v1_r1 < v1_r2);
    }

    #[test]
    fn test_package_version_ord() {
        let v1_2_3a = PackageVersion::new("1.2.3a", Some("_alpha"), None).unwrap();
        let v1_2_3a_p2024 = PackageVersion::new("1.2.3a", Some("_alpha_p20240101"), None).unwrap();
        let v1_2_3a_p2025 = PackageVersion::new("1.2.3a", Some("_alpha_p20251111"), None).unwrap();
        let v1_5_0b = PackageVersion::new("1.5.0b", None, None).unwrap();
        let v1_5_0br3 = PackageVersion::new("1.5.0b", None, Some("3")).unwrap();

        assert!(v1_2_3a < v1_2_3a_p2024);
        assert!(v1_2_3a_p2024 < v1_2_3a_p2025);
        assert!(v1_2_3a_p2025 < v1_5_0b);
        assert!(v1_5_0b < v1_5_0br3);
    }
}
