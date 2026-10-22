use std::path::Path;

use super::entry::{Entry, EntryValue, Precedence};
use super::parse_from_path;
use crate::types::FxHashSet;
use crate::utils::Inherit;

/// Holds entries from a line-based file.
///
/// Lines beginning with a hyphen clear previous entries with the same value.
/// A bare `-*` clears all previous entries. The reset isn't specified by the PMS but
/// needed for Portage compatibility.
#[derive(Clone, Debug)]
pub struct LineEntries<T: EntryValue>(Vec<LineEntry<T>>);

impl<T: EntryValue> LineEntries<T> {
    /// Creates a new [`LineEntries`] from the given `path`.
    ///
    /// if `recursive` is set, `path` is treated as directory and all files in that directory are merged together.
    pub fn from_path(path: &Path, order: Precedence, recursive: bool) -> anyhow::Result<Self> {
        let entries = parse_from_path(path, recursive, true, |entry| {
            Self::parse_entry(entry, order)
        })?;
        Ok(Self::from_entries(entries))
    }

    /// Consumes self and returns the effective entries in their original order.
    ///
    /// A [`LineEntry::Reset`] discards all earlier entries.
    pub fn into_entries(self) -> impl Iterator<Item = Entry<T>> {
        Self::dedupe(self.0.into_iter().rev()).into_iter()
    }

    /// Consumes self and returns an iterator of the inner `T` that remain set.
    ///
    /// Values reverted by a later `-<value>` or `-*` are omitted.
    pub fn finalize(self) -> impl Iterator<Item = T> {
        self.into_entries()
            .filter(|entry| entry.op.as_bool())
            .map(Entry::into_inner)
    }

    /// Appends the parsed `line`, treating a bare `-*` as a reset.
    pub fn push(&mut self, line: &str, order: Precedence) -> anyhow::Result<()> {
        self.0.push(Self::parse_entry(line, order)?);
        Ok(())
    }

    /// Creates a new [`LineEntries`] from parsed `entries`.
    fn from_entries(entries: impl IntoIterator<Item = LineEntry<T>>) -> Self {
        Self(entries.into_iter().collect())
    }

    /// Parses a single line, treating a bare `-*` as a reset.
    fn parse_entry(entry: &str, order: Precedence) -> anyhow::Result<LineEntry<T>> {
        match entry {
            "-*" => Ok(LineEntry::Reset),
            entry => Entry::from_str(entry, order).map(LineEntry::Value),
        }
    }

    /// Returns the effective entries from `entries`.
    ///
    /// The caller must ensure the entries are in reverse order.
    fn dedupe(entries: impl Iterator<Item = LineEntry<T>>) -> Vec<Entry<T>> {
        let mut seen = FxHashSet::default();
        let mut result = Vec::new();
        for item in entries {
            match item {
                LineEntry::Reset => break,
                LineEntry::Value(entry) => {
                    if seen.insert(entry.inner().clone()) {
                        result.push(entry);
                    }
                }
            }
        }
        result.reverse();
        result
    }

    /// Creates a new [`LineEntries`] from the given `lines`.
    #[cfg(test)]
    pub fn from_lines<'a>(
        lines: impl IntoIterator<Item = &'a str>,
        order: Precedence,
    ) -> anyhow::Result<Self> {
        let entries = lines
            .into_iter()
            .map(|entry| Self::parse_entry(entry, order))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self::from_entries(entries))
    }
}

impl<T: EntryValue> Inherit for LineEntries<T> {
    fn inherit_from(&mut self, parent: &LineEntries<T>) -> anyhow::Result<()> {
        self.0 = Self::dedupe(self.0.iter().rev().chain(parent.0.iter().rev()).cloned())
            .into_iter()
            .map(LineEntry::Value)
            .collect();
        Ok(())
    }
}

impl<T: EntryValue> Default for LineEntries<T> {
    fn default() -> Self {
        Self(Vec::default())
    }
}

/// A single line of a [`LineEntries`] file, either a value or a `-*` reset.
#[derive(Clone, Debug)]
enum LineEntry<T: EntryValue> {
    Value(Entry<T>),
    Reset,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::{AtomEntries, UseEntries};

    #[test]
    fn test_entries() -> anyhow::Result<()> {
        let content = [
            "dev-libs/libffi",
            "app-arch/xz-utils",
            "app-arch/zstd",
            "-app-arch/rpm",
        ];

        let file = AtomEntries::from_lines(content, Precedence::Repository)?;
        assert_eq!(
            file.into_entries().collect::<Vec<_>>(),
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
        let grand_parent = AtomEntries::from_lines(
            [
                "dev-libs/libffi",
                "app-arch/xz-utils",
                "app-arch/zstd",
                "app-arch/rpm",
            ],
            Precedence::Repository,
        )?;

        let parent =
            AtomEntries::from_lines(["-app-arch/rpm", "-sys-libs/glibc"], Precedence::Profile(0))?;

        let mut child = AtomEntries::from_lines(
            [
                "-app-arch/xz-utils",
                "app-arch/zstd",
                "app-arch/rpm",
                "-app-arch/rpm",
            ],
            Precedence::Profile(1),
        )?;

        child.inherit_from(&parent.inherit(&grand_parent)?)?;

        assert_eq!(
            child.into_entries().collect::<Vec<_>>(),
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

    #[test]
    fn test_inherit_reset() -> anyhow::Result<()> {
        let grand_parent = UseEntries::from_lines("foo\nbar\n".lines(), Precedence::Profile(0))?;
        let parent = UseEntries::from_lines("-*\nbaz\n".lines(), Precedence::Profile(1))?;
        let mut child = UseEntries::from_lines("qux\n".lines(), Precedence::Profile(2))?;

        child.inherit_from(&parent.inherit(&grand_parent)?)?;
        assert_eq!(
            child.into_entries().collect::<Vec<_>>(),
            vec![
                Entry::from_str("baz", Precedence::Profile(1))?,
                Entry::from_str("qux", Precedence::Profile(2))?,
            ]
        );

        let parent = UseEntries::from_lines("foo\nbar\n".lines(), Precedence::Profile(0))?;
        let mut child = UseEntries::from_lines("baz\n-*\nqux\n".lines(), Precedence::Profile(1))?;
        child.inherit_from(&parent)?;
        assert_eq!(
            child.into_entries().collect::<Vec<_>>(),
            vec![Entry::from_str("qux", Precedence::Profile(1))?]
        );
        Ok(())
    }
}
