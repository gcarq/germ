use crate::files::FileFromPath;
use crate::files::entry::{FileEntry, Prefixed};
use crate::types::FxHashSet;
use crate::utils::Inherit;
use anyhow::{Context, Result};

/// Represents a one-item-per-line file.
///
/// EAPI > 6 supports directories, in that case all files in that directory are merged together.
/// Lines beginning with a hyphen clear the content of previous lines that are equal to the
/// remainder of that line.
/// TODO: consider saving relevant file path and line numbers for better error messages.
#[derive(Clone)]
#[cfg_attr(test, derive(Debug))]
pub struct LineBasedFile<T: FileEntry>(Vec<Prefixed<T>>);

impl<T: FileEntry> LineBasedFile<T> {
    pub fn iter(&self) -> impl Iterator<Item = &Prefixed<T>> {
        self.0.iter()
    }

    pub fn into_iter(self) -> impl Iterator<Item = Prefixed<T>> {
        self.0.into_iter()
    }

    /// Consumes `self` and returns a vector of all entries that should be set.
    pub fn finalize(self) -> Vec<T> {
        self.0
            .into_iter()
            .filter_map(Prefixed::into_value)
            .collect()
    }
}

impl<T: FileEntry> FileFromPath for LineBasedFile<T> {
    /// Creates a new instance from the given `content`.
    /// Lines that are empty or start with `#` are ignored.
    fn from_string(content: String) -> Result<Self>
    where
        Self: Sized,
    {
        let lines = content
            .lines()
            .enumerate()
            .map(|(lineno, line)| (lineno, line.trim()))
            .filter(|(_, line)| !line.is_empty() && !line.starts_with('#'))
            .map(|(lineno, line)| {
                line.parse()
                    .with_context(|| format!("failed to parse line {}: {line}", lineno + 1))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self(lines))
    }
}

impl<T: FileEntry> Default for LineBasedFile<T> {
    fn default() -> Self {
        Self(Vec::default())
    }
}

impl<T: FileEntry> Inherit for LineBasedFile<T> {
    fn inherit_from(&mut self, parent: &LineBasedFile<T>) {
        let mut seen = FxHashSet::default();
        let mut result = Vec::new();
        for item in self.0.iter().rev().chain(parent.0.iter().rev()) {
            if seen.insert(item.inner().clone()) {
                result.push(item.clone());
            }
        }
        self.0 = result;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::PackageEntries;
    use crate::files::entry::Prefixed::{Set, Unset};

    #[test]
    fn test_from_string() -> Result<()> {
        let content = "
            dev-libs/libffi

            # this is a comment
            app-arch/xz-utils
            app-arch/zstd
            -app-arch/rpm
        ";

        let file = PackageEntries::from_string(content.into())?;
        assert_eq!(
            file.0,
            vec![
                "dev-libs/libffi".parse()?,
                "app-arch/xz-utils".parse()?,
                "app-arch/zstd".parse()?,
                "-app-arch/rpm".parse()?,
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
        )?;

        let parent = PackageEntries::from_string(
            "
            dev-libs/libffi
            -app-arch/rpm
            -sys-libs/glibc
            "
            .into(),
        )?;

        let mut child = PackageEntries::from_string(
            "
            -app-arch/xz-utils
            app-arch/zstd
            app-arch/rpm
            -app-arch/rpm
            "
            .into(),
        )?;

        child.inherit_from(&parent.inherit(&grand_parent));

        assert_eq!(
            child.0,
            vec![
                Unset("app-arch/rpm".parse()?),
                Set("app-arch/zstd".parse()?),
                Unset("app-arch/xz-utils".parse()?),
                Set("dev-libs/libffi".parse()?),
                Unset("sys-libs/glibc".parse()?),
            ]
        );
        Ok(())
    }
}
