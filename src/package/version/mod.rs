use crate::package::version::suffix::VersionSuffixes;
use anyhow::Result;
use number::VersionNumber;
use std::fmt;
use std::str::FromStr;

pub mod number;
pub mod suffix;

/// Represents a package version according to PMS section 3.2 and 3.3.
/// This includes the base version components (e.g., "1.2.3a"), any suffixes
/// (e.g., "_alpha1", "_p20240101"), and the revision number (e.g., "-r1").
#[derive(Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct PackageVersion {
    pub number: VersionNumber,
    pub suffixes: VersionSuffixes,
    pub revision: usize,
}

impl PackageVersion {
    /// Creates a new [`PackageVersion`] from the given version string, suffixes, and revision.
    pub fn new(version: &str, suffixes: Option<&str>, revision: usize) -> Result<Self> {
        Ok(Self {
            number: VersionNumber::from_str(version)?,
            suffixes: match suffixes {
                Some(s) => VersionSuffixes::from_str(s)?,
                None => VersionSuffixes::default(),
            },
            revision,
        })
    }
}

impl fmt::Display for PackageVersion {
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
    fn test_package_version_display_simple() {
        assert_eq!(
            PackageVersion::new("1.0.0", None, 0,).unwrap().to_string(),
            "1.0.0"
        );
    }

    #[test]
    fn test_package_version_display_complex() {
        assert_eq!(
            PackageVersion::new("2.3.4a", Some("alpha3_p20250101"), 2,)
                .unwrap()
                .to_string(),
            "2.3.4a_alpha3_p20250101-r2"
        );
    }

    #[test]
    fn test_package_version_ord_suffixes() {
        let v1_2_3_alpha = PackageVersion::new("1.2.3", Some("_alpha"), 0).unwrap();
        let v1_2_3_alpha_p2025 = PackageVersion::new("1.2.3", Some("_alpha_p2025"), 0).unwrap();
        let v1_2_3_beta = PackageVersion::new("1.2.3", Some("_beta"), 0).unwrap();
        let v1_2_3_patch = PackageVersion::new("1.2.3", Some("_p"), 0).unwrap();

        assert!(v1_2_3_alpha < v1_2_3_alpha_p2025);
        assert!(v1_2_3_alpha < v1_2_3_beta);
        assert!(v1_2_3_beta < v1_2_3_patch);
    }

    #[test]
    fn test_package_version_ord_revision() {
        let v1_r0 = PackageVersion::new("1.0.0", None, 0).unwrap();
        let v1_r1 = PackageVersion::new("1.0.0", None, 1).unwrap();
        let v1_r2 = PackageVersion::new("1.0.0", None, 2).unwrap();

        assert!(v1_r0 < v1_r1);
        assert!(v1_r1 < v1_r2);
    }

    #[test]
    fn test_package_version_ord_complex() {
        let v1_2_3a_p2024 = PackageVersion::new("1.2.3a", Some("_alpha_p20240101"), 0).unwrap();
        let v1_2_3a_p2025 = PackageVersion::new("1.2.3a", Some("_alpha_p20251111"), 0).unwrap();
        let v1_5_0b = PackageVersion::new("1.5.0b", None, 0).unwrap();

        assert!(v1_2_3a_p2024 < v1_2_3a_p2025);
        assert!(v1_2_3a_p2025 < v1_5_0b);
    }
}
