mod dfs;

pub(crate) use self::dfs::{DfsState, Visit};

use anyhow::{anyhow, bail};
use md5::{Digest, Md5};
use std::fmt::Write;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// Removes a trailing comment from the given `line` and returns a trimmed str.
pub fn strip_line_comment(line: &str) -> &str {
    line.split_once('#')
        .map_or(line, |(content, _)| content)
        .trim()
}

/// Checks whether a line is blank or a comment.
pub fn is_blank_or_comment(line: &str) -> bool {
    strip_line_comment(line).is_empty()
}

/// Trait for inheriting configurations from another instance.
pub trait Inherit {
    fn inherit_from(&mut self, parent: &Self) -> anyhow::Result<()>;

    /// Inherits the configuration of the given parent into self and returns the result as a new
    /// instance.
    #[must_use = "this method returns the inherited instance"]
    fn inherit(self, parent: &Self) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let mut child = self;
        child.inherit_from(parent)?;
        Ok(child)
    }
}

/// Calculates and returns the MD5 hash of the given `file` as a hexadecimal `String`.
#[allow(unused)]
pub fn md5sum(data: &[u8]) -> anyhow::Result<String> {
    let hash = Md5::digest(data);
    let mut checksum = String::with_capacity(hash.len() * 2);

    for byte in hash {
        write!(checksum, "{byte:02x}")?;
    }

    Ok(checksum)
}

/// Uses [`shlex`] to analyze and split the given [`String`] into key-value pairs.
pub fn shlex_split(content: String) -> anyhow::Result<Vec<(String, String)>> {
    shlex::split(&content)
        .ok_or_else(|| anyhow!("Unable to split text due to syntax errors"))?
        .into_iter()
        .map(|line| match line.split_once('=') {
            Some((key, value)) => Ok((key.to_string(), value.to_string())),
            None => bail!("syntax error in file: {line}"),
        })
        .collect()
}

/// Reads all files for the given `path` and returns a sorted `Vec`.
///
/// This function ignores subdirectories and files starting
/// with `.` or ending with `~`.
pub fn list_files(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = fs::read_dir(path)?
        .map(|entry| {
            let entry = entry?;
            let file_name = entry.file_name();
            let file_name = file_name.as_bytes();
            if file_name.starts_with(b".") || file_name.ends_with(b"~") {
                return Ok(None);
            }
            Ok(entry.file_type()?.is_file().then(|| entry.path()))
        })
        .filter_map(Result::transpose)
        .collect::<anyhow::Result<Vec<_>>>()?;

    files.sort_unstable_by(|a, b| a.file_name().cmp(&b.file_name()));

    Ok(files)
}

/// Fallible variant of [`Iterator::all()`]
pub fn try_all<I, F, E>(iter: I, mut func: F) -> Result<bool, E>
where
    I: IntoIterator,
    F: FnMut(I::Item) -> Result<bool, E>,
{
    for item in iter {
        if !func(item)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Fallible variant of [`Iterator::any()`].
pub fn try_any<I, F, E>(iter: I, mut func: F) -> Result<bool, E>
where
    I: IntoIterator,
    F: FnMut(I::Item) -> Result<bool, E>,
{
    for item in iter {
        if func(item)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_files() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        fs::write(temp.path().join("b"), "")?;
        fs::write(temp.path().join("a"), "")?;
        fs::write(temp.path().join(".hidden"), "")?;
        fs::write(temp.path().join("backup~"), "")?;
        fs::create_dir(temp.path().join("directory"))?;

        let files = list_files(temp.path())?;
        assert_eq!(files, vec![temp.path().join("a"), temp.path().join("b")]);
        Ok(())
    }

    #[test]
    fn test_is_blank_or_comment() {
        for (line, expected) in [
            ("", true),
            ("  ", true),
            ("\t# comment", true),
            ("value", false),
            ("value # comment", false),
        ] {
            assert_eq!(is_blank_or_comment(line), expected, "{line:?}");
        }
    }

    #[test]
    fn test_strip_line_comment() {
        assert_eq!(strip_line_comment("value # comment"), "value");
    }

    #[test]
    fn test_shlex_split_valid_syntax() {
        let content = r#"
            # This is a comment
            VAR1=value1
            VAR2="value with spaces"

            VAR3='another value'
            VAR4=value_with_\"escaped_quotes\"
        "#;

        let result = shlex_split(content.into()).unwrap();
        let expected = vec![
            ("VAR1".into(), "value1".into()),
            ("VAR2".into(), "value with spaces".into()),
            ("VAR3".into(), "another value".into()),
            ("VAR4".into(), r#"value_with_"escaped_quotes""#.into()),
        ];

        assert_eq!(result, expected);
    }

    #[test]
    fn test_shlex_split_error() {
        let content = r#"
            # This is a comment
            VAR1=value1
            INVALID_LINE
        "#;

        assert!(shlex_split(content.to_string()).is_err());
    }

    #[test]
    fn test_md5sum() {
        let hash = md5sum("The quick brown fox jumps over the lazy dog".as_bytes()).unwrap();
        assert_eq!(hash, "9e107d9d372bb6826bd81d3542a419d6");
    }
}
