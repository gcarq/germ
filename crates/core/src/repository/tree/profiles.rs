use crate::files::content_from_path;
use anyhow::anyhow;
use std::ops::Deref;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
#[error(transparent)]
pub struct ProfileError(#[from] anyhow::Error);

/// Holds all profile descriptions as found in `profiles/profiles.desc`.
#[derive(Default, Debug)]
pub struct ProfileDescriptions(Vec<ProfileDescription>);

impl ProfileDescriptions {
    pub fn from_path(path: &Path) -> Result<Self, ProfileError> {
        let content = content_from_path(path, false, true)?;
        let descriptions = content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(ProfileDescription::from_line)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self(descriptions))
    }
}

impl Deref for ProfileDescriptions {
    type Target = Vec<ProfileDescription>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Represents a profile description as found in profiles.desc file.
#[derive(Debug)]
pub struct ProfileDescription {
    pub keyword: String,
    pub profile_path: String,
    #[allow(unused)]
    pub stability: String,
}

impl ProfileDescription {
    /// Parses a profile description from a single line.
    /// The line must consist of `<keyword> <profile_path> <stability>` otherwise an Err is returned.
    pub fn from_line(line: &str) -> Result<Self, ProfileError> {
        let parts = line.split_ascii_whitespace().collect::<Vec<_>>();
        match parts.as_slice() {
            [keyword, profile_path, stability] => Ok(Self {
                keyword: (*keyword).to_owned(),
                profile_path: (*profile_path).to_owned(),
                stability: (*stability).to_owned(),
            }),
            _ => Err(ProfileError(anyhow!(
                "Invalid profile description line: {line}"
            ))),
        }
    }
}

/// Holds all supported architectures from `profiles/arch.list`.
#[derive(Default, Debug)]
pub struct ArchList(Vec<String>);

impl ArchList {
    pub fn from_path(path: &Path) -> Result<Self, ProfileError> {
        let content = content_from_path(path, false, true)?;
        let archs = content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(String::from)
            .collect();
        Ok(Self(archs))
    }

    /// Checks if the given `arch` is supported.
    pub fn supports(&self, arch: &str) -> bool {
        self.0.iter().any(|a| a == arch)
    }
}

impl Deref for ArchList {
    type Target = Vec<String>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
