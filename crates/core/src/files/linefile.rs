use crate::files::content_from_path;
use crate::files::entry::{Entry, FileEntry, Precedence};
use crate::types::FxHashSet;
use crate::utils::{Inherit, is_blank_or_comment};
use anyhow::{Context, Result};
use std::path::Path;

/// Represents a one-item-per-line file.
///
/// EAPI > 6 supports directories, in that case all files in that directory are merged together.
/// Lines beginning with a hyphen clear the content of previous lines that are equal to the
/// remainder of that line.
/// TODO: consider saving relevant file path and line numbers for better error messages.
#[derive(Clone, Debug)]
pub struct LineBasedFile<T: FileEntry>(Vec<Entry<T>>);

impl<T: FileEntry> LineBasedFile<T> {
    pub fn from_path(path: &Path, order: Precedence, recursive: bool) -> Result<Self> {
        let content = content_from_path(path, recursive, true)?;
        Self::from_string(content, order)
    }

    pub fn from_string(content: String, order: Precedence) -> Result<Self> {
        let lines = content
            .lines()
            .enumerate()
            .map(|(lineno, line)| (lineno, line.trim()))
            .filter(|(_, line)| !is_blank_or_comment(line))
            .map(|(lineno, line)| {
                Entry::from_str(line, order)
                    .with_context(|| format!("failed to parse line {}: {line}", lineno + 1))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self(lines))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Entry<T>> {
        self.0.iter()
    }

    pub fn into_iter(self) -> impl Iterator<Item = Entry<T>> {
        self.0.into_iter()
    }
}

impl<T: FileEntry> Default for LineBasedFile<T> {
    fn default() -> Self {
        Self(Vec::default())
    }
}

impl<T: FileEntry> Inherit for LineBasedFile<T> {
    fn inherit_from(&mut self, parent: &LineBasedFile<T>) -> anyhow::Result<()> {
        let mut seen = FxHashSet::default();
        let mut result = Vec::new();
        for item in self.0.iter().rev().chain(parent.0.iter().rev()) {
            if seen.insert(item.inner()) {
                result.push(item.clone());
            }
        }
        result.reverse();
        self.0 = result;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::PackageEntries;

    #[test]
    fn test_from_string() -> Result<()> {
        let content = "
            dev-libs/libffi

            # this is a comment
            app-arch/xz-utils
            app-arch/zstd
            -app-arch/rpm
        ";

        let file = PackageEntries::from_string(content.into(), Precedence::Repository)?;
        assert_eq!(
            file.0,
            vec![
                Entry::from_str("dev-libs/libffi", Precedence::Repository)?,
                Entry::from_str("app-arch/xz-utils", Precedence::Repository)?,
                Entry::from_str("app-arch/zstd", Precedence::Repository)?,
                Entry::from_str("-app-arch/rpm", Precedence::Repository)?,
            ]
        );
        Ok(())
    }

    #[test]
    fn test_inherit_from() -> Result<()> {
        let grand_parent = PackageEntries::from_string(
            "
            dev-libs/libffi
            app-arch/xz-utils
            app-arch/zstd
            app-arch/rpm
        "
            .into(),
            Precedence::Repository,
        )?;

        let parent = PackageEntries::from_string(
            "
            -app-arch/rpm
            -sys-libs/glibc
            "
            .into(),
            Precedence::Profile(0),
        )?;

        let mut child = PackageEntries::from_string(
            "
            -app-arch/xz-utils
            app-arch/zstd
            app-arch/rpm
            -app-arch/rpm
            "
            .into(),
            Precedence::Profile(1),
        )?;

        child.inherit_from(&parent.inherit(&grand_parent)?)?;

        assert_eq!(
            child.0,
            vec![
                Entry::from_str("dev-libs/libffi", Precedence::Repository)?,
                Entry::from_str("-sys-libs/glibc", Precedence::Profile(0))?,
                Entry::from_str("-app-arch/xz-utils", Precedence::Profile(1))?,
                Entry::from_str("app-arch/zstd", Precedence::Profile(1))?,
                Entry::from_str("-app-arch/rpm", Precedence::Profile(1))?,
            ]
        );
        Ok(())
    }
}
