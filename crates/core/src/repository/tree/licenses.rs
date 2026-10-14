use std::ffi::OsStr;
use std::ops::Deref;
use std::path::Path;

use anyhow::Context;
use germ_pms::LicenseName;
use log::warn;

use crate::types::FxHashSet;
use crate::utils::list_files;

/// Holds all known license names from the `licenses` directory.
#[derive(Debug, Default)]
pub struct Licenses(FxHashSet<LicenseName>);

impl Licenses {
    /// Creates a new [`Licenses`] collection from the given `path`.
    ///
    /// A missing directory yields an empty collection and
    /// invalid licenses are reported and skipped.
    pub fn from_path(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let mut licenses = FxHashSet::default();
        for file in list_files(path)? {
            let name = file
                .file_name()
                .and_then(OsStr::to_str)
                .with_context(|| format!("invalid license file name in {}", path.display()))?;
            match LicenseName::new(name) {
                Ok(name) => {
                    licenses.insert(name);
                }
                Err(err) => warn!(
                    "invalid license file in {}: {err}. Skipping...",
                    path.display()
                ),
            }
        }
        Ok(Self(licenses))
    }

    /// Extends `Self` with the given `other`.
    pub fn extend(&mut self, other: &Self) {
        self.0.extend(other.0.iter().cloned());
    }
}

impl Deref for Licenses {
    type Target = FxHashSet<LicenseName>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn test_licenses_missing_dir() {
        let temp = tempfile::tempdir().unwrap();
        let licenses = Licenses::from_path(&temp.path().join("licenses")).unwrap();
        assert!(licenses.is_empty());
    }

    #[test]
    fn test_licenses_collects_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("licenses");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("GPL-3"), "").unwrap();
        fs::write(path.join("MIT"), "").unwrap();
        fs::write(path.join("invalid license"), "").unwrap();
        fs::create_dir(path.join("nested")).unwrap();

        let licenses = Licenses::from_path(&path).unwrap();
        assert!(licenses.contains("GPL-3"));
        assert!(licenses.contains("MIT"));
        assert_eq!(licenses.len(), 2);
    }
}
