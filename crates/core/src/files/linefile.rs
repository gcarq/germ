use std::path::Path;

use super::entry::{Entry, EntryValue, LineOrigin, Precedence};
use super::parse_from_path;
#[cfg(test)]
use super::test_support::with_origins;
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
        let entries = parse_from_path(path, recursive, true, |text, source| {
            Self::parse_entry(text, order, source)
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
    pub fn push(
        &mut self,
        line: &str,
        order: Precedence,
        origin: LineOrigin,
    ) -> anyhow::Result<()> {
        self.0.push(Self::parse_entry(line, order, origin)?);
        Ok(())
    }

    /// Creates a new [`LineEntries`] from parsed `entries`.
    fn from_entries(entries: impl IntoIterator<Item = LineEntry<T>>) -> Self {
        Self(entries.into_iter().collect())
    }

    /// Parses a single line, treating a bare `-*` as a reset.
    fn parse_entry(
        line: &str,
        order: Precedence,
        origin: LineOrigin,
    ) -> anyhow::Result<LineEntry<T>> {
        match line {
            "-*" => Ok(LineEntry::Reset),
            line => Entry::new(line, order, origin).map(LineEntry::Value),
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
        let entries = with_origins(lines)
            .map(|(line, origin)| Self::parse_entry(line, order, origin))
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
    use crate::files::test_support::entry;
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
                entry("dev-libs/libffi", Precedence::Repository, 1),
                entry("app-arch/xz-utils", Precedence::Repository, 2),
                entry("app-arch/zstd", Precedence::Repository, 3),
                entry("-app-arch/rpm", Precedence::Repository, 4),
            ]
        );
        Ok(())
    }

    #[test]
    fn test_inherit() -> anyhow::Result<()> {
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
                entry("dev-libs/libffi", Precedence::Repository, 1),
                entry("-sys-libs/glibc", Precedence::Profile(0), 2),
                entry("-app-arch/xz-utils", Precedence::Profile(1), 1),
                entry("app-arch/zstd", Precedence::Profile(1), 2),
                entry("-app-arch/rpm", Precedence::Profile(1), 4),
            ]
        );

        // A reset in an inherited parent discards its own ancestors.
        let grand_parent = UseEntries::from_lines("foo\nbar\n".lines(), Precedence::Profile(0))?;
        let parent = UseEntries::from_lines("-*\nbaz\n".lines(), Precedence::Profile(1))?;
        let mut child = UseEntries::from_lines("qux\n".lines(), Precedence::Profile(2))?;

        child.inherit_from(&parent.inherit(&grand_parent)?)?;
        assert_eq!(
            child.into_entries().collect::<Vec<_>>(),
            vec![
                entry("baz", Precedence::Profile(1), 2),
                entry("qux", Precedence::Profile(2), 1),
            ]
        );

        // A reset in the child discards everything inherited.
        let parent = UseEntries::from_lines("foo\nbar\n".lines(), Precedence::Profile(0))?;
        let mut child = UseEntries::from_lines("baz\n-*\nqux\n".lines(), Precedence::Profile(1))?;
        child.inherit_from(&parent)?;
        assert_eq!(
            child.into_entries().collect::<Vec<_>>(),
            vec![entry("qux", Precedence::Profile(1), 3)]
        );
        Ok(())
    }
}
