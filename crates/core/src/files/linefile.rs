use crate::files::content_from_path;
use crate::files::entry::{Entry, EntryValue, Precedence};
use crate::types::FxHashSet;
use crate::utils::{Inherit, strip_line_comment};
use anyhow::Context;
use std::path::Path;

/// Holds entries from a line-based file.
///
/// Lines beginning with a hyphen clear the content of previous lines that are equal to the
/// remainder of that line.
/// TODO: consider saving relevant file path and line numbers for better error messages.
#[derive(Clone, Debug)]
pub struct LineEntries<T: EntryValue>(Vec<Entry<T>>);

impl<T: EntryValue> LineEntries<T> {
    /// Creates a new [`LineEntries`] from the given `path`.
    ///
    /// if `recursive` is set, `path` is treated as directory and all files in that directory are merged together.
    pub fn from_path(path: &Path, order: Precedence, recursive: bool) -> anyhow::Result<Self> {
        let content = content_from_path(path, recursive, true)?;
        Self::from_string(content, order)
            .with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn from_string(content: String, order: Precedence) -> anyhow::Result<Self> {
        let entries = content
            .lines()
            .enumerate()
            .map(|(lineno, entry)| (lineno + 1, strip_line_comment(entry)))
            .filter(|(_, entry)| !entry.is_empty())
            .map(|(lineno, entry)| {
                Entry::from_str(entry, order)
                    .with_context(|| format!("error in line {lineno}: {entry}"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self(entries))
    }

    /// Consumes self and returns an iterator of [`Entry<T>`].
    pub fn into_iter(self) -> impl Iterator<Item = Entry<T>> {
        self.0.into_iter()
    }

    /// Consumes self and returns an iterator of inner `T`.
    pub fn into_inner(self) -> impl Iterator<Item = T> {
        self.into_iter().map(Entry::into_inner)
    }
}

impl<T: EntryValue> Default for LineEntries<T> {
    fn default() -> Self {
        Self(Vec::default())
    }
}

impl<T: EntryValue> Inherit for LineEntries<T> {
    fn inherit_from(&mut self, parent: &LineEntries<T>) -> anyhow::Result<()> {
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
    use crate::files::PackageEntries;

    use super::*;

    #[test]
    fn test_from_string() -> anyhow::Result<()> {
        let content = "
            dev-libs/libffi # inline comment

            # this is a comment
            app-arch/xz-utils # added because of XY
            app-arch/zstd
            -app-arch/rpm # removal comment
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
    fn test_inherit_from() -> anyhow::Result<()> {
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
