use std::path::{Path, PathBuf};
use std::{fmt, fs, io};

use tempfile::TempDir;

use crate::SysConf;

/// Builds a [`SysConf`] with paths rooted in `temp_dir`.
pub fn build_sysconf(temp_dir: &TempDir) -> SysConf {
    SysConf::new(temp_dir.path().to_owned(), temp_dir.path().join(".cache"))
}

/// Asserts that both iterables contain the same [`fmt::Display`] values.
#[track_caller]
pub fn assert_display_eq<A, B>(actual: A, expected: B)
where
    A: IntoIterator,
    B: IntoIterator,
    A::Item: fmt::Display,
    B::Item: fmt::Display,
{
    let actual = actual.into_iter().map(|value| value.to_string());
    let expected = expected.into_iter().map(|value| value.to_string());
    assert_eq!(actual.collect::<Vec<_>>(), expected.collect::<Vec<_>>());
}

/// Writes `contents` to `path`, creating parent directories if needed.
pub fn write_file(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

/// A file or file-directory entry in a fixture tree.
pub enum FixtureFile {
    File { path: PathBuf, contents: String },
    Directory { path: PathBuf, contents: String },
}

impl FixtureFile {
    /// Creates a file at `path`.
    pub fn file(path: impl Into<PathBuf>, contents: impl Into<String>) -> Self {
        Self::File {
            path: path.into(),
            contents: contents.into(),
        }
    }

    /// Creates a `path/entries` file-directory entry.
    pub fn entries_dir(path: impl Into<PathBuf>, contents: impl Into<String>) -> Self {
        Self::Directory {
            path: path.into(),
            contents: contents.into(),
        }
    }

    /// Writes the entry below `root`.
    pub fn write_to(&self, root: &Path) -> io::Result<()> {
        match self {
            Self::File { path, contents } => write_file(root.join(path), contents),
            Self::Directory { path, contents } => {
                write_file(root.join(path).join("entries"), contents)
            }
        }
    }
}
