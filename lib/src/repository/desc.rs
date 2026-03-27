use crate::files::FileFromPath;
use anyhow::{Result, anyhow};
use std::ops::Deref;

/// Holds all profile descriptions as found in `profiles/profiles.desc`.
#[derive(Default)]
#[cfg_attr(test, derive(Debug))]
pub struct ProfileDescriptions(Vec<ProfileDescription>);

impl FileFromPath for ProfileDescriptions {
    /// Creates a new instance from the given `content`.
    /// Lines that are empty or start with `#` are ignored.
    fn from_string(content: String) -> Result<Self>
    where
        Self: Sized,
    {
        let descriptions = content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(ProfileDescription::from_line)
            .collect::<Result<Vec<_>>>()?;
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
#[cfg_attr(test, derive(Debug))]
pub struct ProfileDescription {
    pub keyword: String,
    pub profile_path: String,
    pub stability: String,
}

impl ProfileDescription {
    /// Parses a profile description from a single line.
    /// The line must consist of `<keyword> <profile_path> <stability>` otherwise an Err is returned.
    pub fn from_line(line: &str) -> Result<Self> {
        let parts = line.split_ascii_whitespace().collect::<Vec<_>>();
        match parts.as_slice() {
            [keyword, profile_path, stability] => Ok(Self {
                keyword: (*keyword).to_owned(),
                profile_path: (*profile_path).to_owned(),
                stability: (*stability).to_owned(),
            }),
            _ => Err(anyhow!("Invalid profile description line: {line}")),
        }
    }
}
