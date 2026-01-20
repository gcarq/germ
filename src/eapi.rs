use crate::consts::SUPPORTED_EAPIS;
use anyhow::Result;
use std::fmt;

/// An EAPI can be thought of as a ‘version’ of the PMS to which a package conforms.
/// See PMS section 2 for more details.
#[derive(Eq, PartialEq, Debug)]
pub struct Eapi {
    pub version: String,
    pub profile_file_dirs: bool,
}

impl Eapi {
    /// Creates a new instance from the given EAPI `version`.
    /// Returns an `Err` if the version is unsupported.
    pub fn new(version: &str) -> Result<Self> {
        if !SUPPORTED_EAPIS.contains(&version) {
            return Err(anyhow::anyhow!("unsupported EAPI: {version}"));
        };

        Ok(Self {
            version: version.to_owned(),
            profile_file_dirs: matches!(version, "7" | "8" | "9"),
        })
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
