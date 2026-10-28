use std::ops::Deref;
use std::path::Path;

use anyhow::anyhow;
use germ_pms::Arch;
use thiserror::Error;

use crate::files::parse_from_path;

#[derive(Debug, Error)]
#[error(transparent)]
pub struct ProfileError(#[from] anyhow::Error);

/// Holds all profile descriptions as found in `profiles/profiles.desc`.
#[derive(Default, Debug)]
pub struct ProfileDescriptions(Vec<ProfileDescription>);

impl ProfileDescriptions {
    pub fn from_path(path: &Path) -> anyhow::Result<Self> {
        let descriptions = parse_from_path(path, false, true, |line, _| {
            ProfileDescription::from_line(line)
        })?;
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
    pub arch: Arch,
    pub profile_path: String,
    #[expect(unused)]
    stability: String,
}

impl ProfileDescription {
    /// Parses a profile description from a single line.
    /// The line must consist of `<arch> <profile_path> <stability>` otherwise an Err is returned.
    fn from_line(line: &str) -> anyhow::Result<Self> {
        let parts = line.split_ascii_whitespace().collect::<Vec<_>>();
        match parts.as_slice() {
            [arch, profile_path, stability] => Ok(Self {
                arch: arch.parse()?,
                profile_path: (*profile_path).to_owned(),
                stability: (*stability).to_owned(),
            }),
            _ => Err(anyhow!("Invalid profile description line: {line}")),
        }
    }
}

/// Holds all supported architectures from `profiles/arch.list`.
#[derive(Default, Debug)]
pub struct Arches(Vec<Arch>);

impl Arches {
    pub fn from_path(path: &Path) -> anyhow::Result<Self> {
        let archs = parse_from_path(path, false, true, |line, _| Arch::new(line))?;
        Ok(Self(archs))
    }

    /// Checks if the given `arch` is supported.
    pub fn contains(&self, arch: &Arch) -> bool {
        self.0.contains(arch)
    }

    pub fn extend(&mut self, other: &Self) {
        self.0.extend_from_slice(&other.0);
    }
}
