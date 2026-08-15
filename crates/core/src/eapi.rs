use anyhow::{Context, anyhow};
use rkyv::{Archive, Deserialize, Serialize};
use std::path::Path;
use std::str::FromStr;
use std::{fmt, fs};
use thiserror::Error;

/// An EAPI can be thought of as a ‘version’ of the PMS to which a package conforms.
/// See PMS section 2 for more details.
#[derive(Archive, Serialize, Deserialize, Copy, Clone, Eq, PartialEq, Hash, Default, Debug)]
pub enum Eapi {
    #[default]
    Zero,
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
}

impl Eapi {
    /// Creates a new instance from the EAPI file at the given `path`.
    ///
    /// Returns `Eapi::default()` if no eapi file exists.
    pub fn from_eapi_file(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Eapi::default());
        }
        Ok(fs::read_to_string(path)
            .with_context(|| anyhow!("unable to read eapi file {}", path.display()))?
            .lines()
            .next()
            .ok_or_else(|| anyhow!("empty eapi file {}", path.display()))?
            .parse()?)
    }

    /// Creates a new instance from the given EAPI `version`.
    /// Returns an [`EapiError`] if the version is not recognized.
    fn new(version: &str) -> Result<Self, EapiError> {
        let version = match version {
            "0" => Self::Zero,
            "1" => Self::One,
            "2" => Self::Two,
            "3" => Self::Three,
            "4" => Self::Four,
            "5" => Self::Five,
            "6" => Self::Six,
            "7" => Self::Seven,
            "8" => Self::Eight,
            "9" => Self::Nine,
            value => return Err(EapiError::Unrecognized(value.to_owned())),
        };
        Ok(version)
    }

    /// Returns `true` if this EAPI is supported for ebuilds.
    pub const fn is_supported_for_ebuilds(&self) -> bool {
        matches!(self, Self::Seven | Self::Eight | Self::Nine)
    }

    /// Returns `true` if this EAPI supports the `BDEPEND` metadata variable.
    pub const fn supports_bdepend(&self) -> bool {
        matches!(self, Self::Seven | Self::Eight | Self::Nine)
    }

    /// Returns `true` if this EAPI supports the `IDEPEND` metadata variable.
    pub const fn supports_idepend(&self) -> bool {
        matches!(self, Self::Eight | Self::Nine)
    }

    /// Returns the minimum supported bash version for this EAPI.
    pub const fn supported_bash_version(&self) -> &str {
        match self {
            Self::Zero | Self::One | Self::Two | Self::Three | Self::Four | Self::Five => "3.2",
            Self::Six | Self::Seven => "4.2",
            Self::Eight => "5.0",
            Self::Nine => "5.3",
        }
    }

    /// Returns `true` if this EAPI supports directories for profile files.
    pub const fn supports_profile_file_dirs(&self) -> bool {
        matches!(self, Self::Seven | Self::Eight | Self::Nine)
    }

    /// Returns `true` if this EAPI supports the `hasv` command in ebuilds.
    pub const fn supports_hasv(&self) -> bool {
        matches!(self, Self::Seven)
    }

    /// Returns `true` if this EAPI supports the `hasq` command in ebuilds.
    pub const fn supports_hasq(&self) -> bool {
        matches!(self, Self::Seven)
    }
}

impl FromStr for Eapi {
    type Err = EapiError;

    fn from_str(version: &str) -> Result<Self, Self::Err> {
        Self::new(version)
    }
}

/// Errors returned when parsing an [`Eapi`].
#[derive(Debug, Error)]
pub enum EapiError {
    #[error("unrecognized EAPI '{0}'")]
    Unrecognized(String),
}

impl fmt::Display for Eapi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let version = match self {
            Self::Zero => "0",
            Self::One => "1",
            Self::Two => "2",
            Self::Three => "3",
            Self::Four => "4",
            Self::Five => "5",
            Self::Six => "6",
            Self::Seven => "7",
            Self::Eight => "8",
            Self::Nine => "9",
        };
        f.write_str(version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eapi_new_ok() {
        for version in ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"] {
            let eapi = Eapi::from_str(version);
            assert!(
                eapi.is_ok(),
                "EAPI version '{version}' should be recognized"
            );
            assert_eq!(eapi.unwrap().to_string(), *version);
        }
    }

    #[test]
    fn test_eapi_new_err() {
        let result = Eapi::from_str("abc");
        assert!(matches!(result, Err(EapiError::Unrecognized(value)) if value == "abc"));
    }

    #[test]
    fn test_is_supported_for_ebuilds() {
        let test_cases = vec![
            (Eapi::Zero, false),
            (Eapi::One, false),
            (Eapi::Two, false),
            (Eapi::Three, false),
            (Eapi::Four, false),
            (Eapi::Five, false),
            (Eapi::Six, false),
            (Eapi::Seven, true),
            (Eapi::Eight, true),
            (Eapi::Nine, true),
        ];
        for (eapi, exp_supported) in test_cases {
            assert_eq!(eapi.is_supported_for_ebuilds(), exp_supported);
        }
    }

    #[test]
    fn test_eapi_metadata_dependency_support() {
        // eapi, supports_bdepend, supports_idepend
        let test_cases = [
            (Eapi::Seven, true, false),
            (Eapi::Eight, true, true),
            (Eapi::Nine, true, true),
        ];
        for (eapi, exp_bdepend, exp_idepend) in test_cases {
            assert_eq!(eapi.supports_bdepend(), exp_bdepend);
            assert_eq!(eapi.supports_idepend(), exp_idepend);
        }
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
            let eapi = Eapi::from_str(version).unwrap();
            let bash_version = eapi.supported_bash_version();
            assert_eq!(bash_version, exp_bash_version);
        }
    }
}
