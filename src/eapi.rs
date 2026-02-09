use crate::consts::{SUPPORTED_EBUILD_EAPIS, VALID_EAPIS};
use anyhow::{Result, anyhow};
use std::fmt;

/// An EAPI can be thought of as a ‘version’ of the PMS to which a package conforms.
/// See PMS section 2 for more details.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Eapi {
    pub version: String,
    pub profile_file_dirs: bool,
}

impl Eapi {
    /// Creates a new instance from the given EAPI `version`.
    /// Returns an `Err` if the version is unsupported.
    pub fn new(version: &str) -> Result<Self> {
        if !VALID_EAPIS.contains(&version) {
            return Err(anyhow::anyhow!("unsupported EAPI: {version}"));
        };

        Ok(Self {
            version: version.to_owned(),
            profile_file_dirs: matches!(version, "7" | "8" | "9"),
        })
    }

    /// Returns `true` if this EAPI is supported for ebuilds.
    pub fn is_supported_for_ebuilds(&self) -> bool {
        SUPPORTED_EBUILD_EAPIS.contains(&self.version.as_str())
    }

    /// Returns the minimum supported bash version for this EAPI.
    /// Returns an `Err` if the EAPI version is unknown.
    pub fn supported_bash_version(&self) -> Result<String> {
        let version = match self.version.as_str() {
            "0" | "1" | "2" | "3" | "4" | "5" => "3.2",
            "6" | "7" => "4.2",
            "8" => "5.0",
            "9" => "5.3",
            x => return Err(anyhow!("unable to determine bash version for EAPI '{x}'")),
        };
        Ok(version.to_owned())
    }

    pub fn is_hasv_supported(&self) -> bool {
        self.version == "7"
    }

    pub fn is_hasq_supported(&self) -> bool {
        self.version == "7"
    }
}

impl Default for Eapi {
    fn default() -> Self {
        Self {
            version: "0".to_owned(),
            profile_file_dirs: false,
        }
    }
}

impl fmt::Display for Eapi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eapi_new_ok() {
        for version in VALID_EAPIS.iter() {
            let eapi = Eapi::new(version);
            assert!(eapi.is_ok());
            assert_eq!(eapi.unwrap().version.as_str(), *version);
        }
    }

    #[test]
    fn test_eapi_new_err() {
        let eapi = Eapi::new("abc");
        assert!(eapi.is_err());
    }

    #[test]
    fn test_eapi_supported_bash_version() {
        let test_cases = vec![
            ("0", "3.2"),
            ("1", "3.2"),
            ("2", "3.2"),
            ("3", "3.2"),
            ("4", "3.2"),
            ("5", "3.2"),
            ("6", "4.2"),
            ("7", "4.2"),
            ("8", "5.0"),
            ("9", "5.3"),
        ];
        for (version, exp_bash_version) in test_cases {
            let eapi = Eapi::new(version).unwrap();
            let bash_version = eapi.supported_bash_version().unwrap();
            assert_eq!(bash_version, exp_bash_version);
        }
    }
}
