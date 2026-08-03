use crate::files::content_from_path;
use anyhow::Result;
use std::ops::Deref;
use std::path::Path;

/// Holds all supported architectures from `profiles/arch.list`.
#[derive(Default, Debug)]
pub struct ArchList(Vec<String>);

impl ArchList {
    pub fn from_path(path: &Path) -> Result<Self> {
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
