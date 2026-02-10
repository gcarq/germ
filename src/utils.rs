use anyhow::{Context, Result, anyhow};
use std::fs;
use std::fs::DirEntry;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// Trait for inheriting configurations from another instance.
pub trait Inherit {
    fn inherit_from(&mut self, parent: &Self);

    /// Inherits the configuration of the given parent into self and returns the result as a new
    /// instance.
    #[must_use = "this returns the inherited instance as a new allocation"]
    fn inherit(self, parent: &Self) -> Self
    where
        Self: Sized,
    {
        let mut child = self;
        child.inherit_from(parent);
        child
    }
}

/// Trait for types that can be constructed from file(s) at a given path.
pub trait FileFromPath {
    /// Creates an instance from the file(s) at the given `path`.
    /// The `path` can point to a single file or a directory containing multiple files.
    /// If the `path` is a directory and `recursive` is true, all files in the directory
    /// are concatenated together in order of their filename.
    /// If `optional` is true, the absence of the [`Path`] does not result in an `Err`.
    fn from_path(path: &Path, recursive: bool, optional: bool) -> Result<Self>
    where
        Self: Sized + Default,
    {
        let metadata = match path.metadata() {
            Ok(metadata) => metadata,
            Err(_) if optional => return Ok(Self::default()),
            Err(e) => return Err(anyhow!("unable to access {}: {e}", path.display())),
        };
        if metadata.is_file() {
            let content = fs::read_to_string(path)?;
            return Self::from_file_content(content);
        }

        if !recursive {
            return Err(anyhow!(
                "{} is a directory, but should be a file",
                path.display()
            ));
        }

        let mut paths = files_from_dir(path)?.collect::<Result<Vec<_>>>()?;
        paths.sort();

        let content = paths
            .into_iter()
            .map(|p| {
                fs::read_to_string(&p)
                    .with_context(|| anyhow!("unable to read file {}", p.display()))
            })
            .collect::<Result<Vec<_>>>()?
            .join("\n");
        Self::from_file_content(content)
    }

    /// Creates an instance from the given file `content`.
    fn from_file_content(content: String) -> Result<Self>
    where
        Self: Sized;
}

/// Uses [`shlex`] to analyze and split the given [`String`] into key-value pairs.
pub fn shlex_split(content: String) -> Result<Vec<(String, String)>> {
    shlex::split(&content)
        .ok_or_else(|| anyhow!("Unable to split text due to syntax errors"))?
        .into_iter()
        .map(|line| match line.split_once('=') {
            Some((key, value)) => Ok((key.to_string(), value.to_string())),
            None => Err(anyhow!("syntax error in file: {line}")),
        })
        .collect()
}

/// Reads the path for all files from the given directory `path`,
/// ignoring subdirectories and files starting with `.` or ending with `~`.
pub fn files_from_dir(path: &Path) -> Result<impl Iterator<Item = Result<PathBuf>>> {
    let iter = fs::read_dir(path)
        .with_context(|| anyhow!("unable to read directory: '{}'", path.display()))?
        .filter_map(|entry| match entry {
            Ok(entry) => match is_file(&entry) {
                Ok(true) => match entry.file_name().as_bytes() {
                    [b'.', ..] | [.., b'~'] => None,
                    _ => Some(Ok(entry.path())),
                },
                Ok(false) => None,
                Err(err) => Some(Err(anyhow!(err))),
            },
            Err(err) => Some(Err(anyhow!(err))),
        });
    Ok(iter)
}

/// Extracts the filename from the given path as a `String`.
pub fn path_to_filename(path: &Path) -> Result<&str> {
    path.file_name()
        .ok_or_else(|| anyhow!("path has no filename: '{}'", path.display()))?
        .to_str()
        .ok_or_else(|| anyhow!("filename contains invalid unicode: '{}'", path.display()))
}

/// Determines whether the given directory `entry` is a file.
/// If the file type is a symlink or cannot be determined directly,
/// it falls back to checking the metadata which requires a syscall.
pub fn is_file(entry: &DirEntry) -> Result<bool> {
    let file_type = entry.file_type()?;
    if file_type.is_file() {
        Ok(true)
    } else if file_type.is_dir() {
        Ok(false)
    } else {
        Ok(entry.metadata()?.is_file())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
