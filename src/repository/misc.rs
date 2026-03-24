use crate::files::FileFromPath;
use anyhow::Result;
use std::ops::Deref;

/// Holds all supported architectures from `profiles/arch.list`.
#[derive(Default)]
#[cfg_attr(test, derive(Debug))]
pub struct ArchList(Vec<String>);

impl FileFromPath for ArchList {
    fn from_string(content: String) -> Result<Self>
    where
        Self: Sized,
    {
        let archs = content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(String::from)
            .collect();
        Ok(Self(archs))
    }
}

impl ArchList {
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
