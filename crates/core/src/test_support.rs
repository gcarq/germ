use std::path::Path;
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
