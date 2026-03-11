use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::Path;

/// Information about a deprecated profile.
/// Contains the recommended profile to upgrade to and an additional info that is shown to the user.
pub struct DeprecationInfo {
    pub recommended_profile: String,
    pub info: String,
}

impl DeprecationInfo {
    /// Builds a [`DeprecationInfo`] from the given path to a deprecated file.
    /// Returns Ok(None) if the file doesn't exist.
    pub fn from_path(path: &Path) -> Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(path).with_context(|| "unable to read deprecated file")?;
        Ok(Some(DeprecationInfo::from_string(content)?))
    }

    /// Builds [`DeprecationInfo`] from the given content of a deprecated file.
    fn from_string(content: String) -> Result<Self> {
        let mut lines = content.lines();
        let recommended_profile = lines
            .next()
            .ok_or_else(|| anyhow!("deprecated file is empty"))?
            .to_owned();
        let info = lines.collect::<Vec<_>>().join("\n").trim().to_owned();
        Ok(Self {
            recommended_profile,
            info,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deprecation_info_from_string() {
        let content = "default/linux/amd64/23.0\n\nThis profile is deprecated. Please upgrade.";
        let deprecation_info = DeprecationInfo::from_string(content.to_owned()).unwrap();
        assert_eq!(
            deprecation_info.recommended_profile,
            "default/linux/amd64/23.0"
        );
        assert_eq!(
            deprecation_info.info,
            "This profile is deprecated. Please upgrade."
        );
    }
}
