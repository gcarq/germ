use crate::files::content_from_path;
use crate::grammar::ARCH;
use crate::utils::is_blank_or_comment;
use anyhow::{anyhow, bail};
use fancy_regex::Regex;
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;
use std::path::Path;
use std::sync::LazyLock;
use thiserror::Error;

/// Regex for architecture name validation.
static ARCH_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"\A{ARCH}\z")).unwrap());

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
            .filter(|line| !is_blank_or_comment(line))
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
    stability: String,
}

impl ProfileDescription {
    /// Parses a profile description from a single line.
    /// The line must consist of `<keyword> <profile_path> <stability>` otherwise an Err is returned.
    fn from_line(line: &str) -> Result<Self, ProfileError> {
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

/// Represents a single architecture e.g.: `amd64`.
#[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Arch(Box<str>);

impl Arch {
    /// Creates a new [`Arch`].
    ///
    /// Returns `Err` if the given `arch` is invalid.
    pub fn new(arch: impl Into<Box<str>>) -> anyhow::Result<Self> {
        let arch = arch.into();
        if !ARCH_RE.is_match(arch.as_ref())? {
            bail!("invalid arch: '{arch}'");
        }
        Ok(Self(arch))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Holds all supported architectures from `profiles/arch.list`.
#[derive(Default, Debug)]
pub struct Arches(Vec<Arch>);

impl Arches {
    pub fn from_path(path: &Path) -> Result<Self, ProfileError> {
        let content = content_from_path(path, false, true)?;
        let archs = content
            .lines()
            .map(str::trim)
            .filter(|line| !is_blank_or_comment(line))
            .map(Arch::new)
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self(archs))
    }

    /// Checks if the given `arch` is supported.
    pub fn contains(&self, arch: &str) -> bool {
        self.0.iter().any(|a| a.as_str() == arch)
    }

    pub fn extend(&mut self, other: &Self) {
        self.0.extend_from_slice(&other.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arch_valid() {
        for arch in ["amd64", "arm64", "x86", "riscv"] {
            let parsed = Arch::new(arch).unwrap();
            assert_eq!(parsed.as_str(), arch);
        }
    }

    #[test]
    fn test_arch_invalid() {
        for arch in ["", "-foo", "+foo", ".foo", "foo.bar"] {
            assert!(Arch::new(arch).is_err(), "{arch:?} should be invalid");
        }
    }
}
