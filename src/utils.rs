use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::Path;

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
/// The path can point to a single file or a directory containing multiple files.
/// If the path is a directory and `recursive` is true, all files in the directory
/// are read and their contents are concatenated together.
/// If `optional` is true, the absence of the path does not result in an error.
/// Implementors must provide the `from_file_content` method to handle the actual content parsing.
pub trait FileFromPath {
    fn from_path(path: &Path, recursive: bool, optional: bool) -> Result<Self>
    where
        Self: Sized + Default,
    {
        if !path.exists() {
            return match optional {
                true => Ok(Self::default()),
                false => Err(anyhow!("{} does not exist", path.display())),
            };
        }

        if path.metadata()?.is_file() {
            let content = fs::read_to_string(path)?;
            return Self::from_file_content(content);
        }
        if !recursive {
            return Err(anyhow!(
                "{} is a directory, but should be a file",
                path.display()
            ));
        }

        let mut paths = Vec::new();
        for entry in fs::read_dir(path).with_context(|| "unable to read directory")? {
            let entry = entry?;
            // Ignore subdirectories and hidden files
            if !entry.metadata()?.is_file()
                || entry
                    .file_name()
                    .to_str()
                    .with_context(|| anyhow!("{} doesn't contain valid unicode", path.display()))?
                    .starts_with('.')
            {
                continue;
            }
            paths.push(entry.path());
        }
        paths.sort();

        let content = paths
            .into_iter()
            .map(|path| {
                fs::read_to_string(&path)
                    .with_context(|| format!("unable to read file {}", path.display()))
            })
            .collect::<Result<Vec<_>>>()?
            .join("\n");
        Self::from_file_content(content)
    }

    fn from_file_content(content: String) -> Result<Self>
    where
        Self: Sized;
}

/// Uses [`shlex`] to analyze and split the given [`String`] into key-value pairs.
pub fn shlex_split(content: String) -> Result<Vec<(String, String)>> {
    shlex::split(&content)
        .ok_or_else(|| anyhow!("Unable to split text due to syntax errors"))?
        .into_iter()
        .map(
            |line| match line.splitn(2, '=').collect::<Vec<_>>().as_slice() {
                [key, value] => Ok((key.to_string(), value.to_string())),
                _ => Err(anyhow!("syntax error in file: {line}")),
            },
        )
        .collect()
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

        let result = shlex_split(content.to_string()).unwrap();
        let expected = vec![
            ("VAR1".to_string(), "value1".to_string()),
            ("VAR2".to_string(), "value with spaces".to_string()),
            ("VAR3".to_string(), "another value".to_string()),
            (
                "VAR4".to_string(),
                r#"value_with_"escaped_quotes""#.to_string(),
            ),
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
