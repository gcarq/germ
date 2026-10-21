//! This module contains common logic to handle file types often used in [`Repository`],
//! [`Profile`], etc.
//!
//! The most used one is [`LineEntries`], which is expressed as text file where each line holds
//! a value. [`LineEntries`] can be inherited and any value prefixed with a hyphen negates
//! the same previous defined item, e.g.: `package.use`, `use.mask`, etc..
//! A bare `-*` line clears all previously entries.
pub mod entry;
mod linefile;
pub mod pkgfile;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use germ_pms::{Atom, UseFlag};
use linefile::LineEntries;

use self::entry::SysAtom;
use crate::utils::{self, strip_line_comment};

/// [`LineEntries`] holding package atoms.
pub type AtomEntries = LineEntries<Atom>;
/// [`LineEntries`] holding system package atoms.
pub type SysAtomEntries = LineEntries<SysAtom>;
/// [`LineEntries`] holding USE flags.
pub type UseEntries = LineEntries<UseFlag>;

/// Reads the content from the given file or folder `path`.
///
/// The `path` can point to a single file or a directory containing multiple files.
/// If the `path` is a directory and `recursive` is true, all files in the directory
/// are concatenated together in order of their filename.
/// If `optional` is true, the absence of the [`Path`] does not result in an `Err`.
pub fn content_from_path(path: &Path, recursive: bool, optional: bool) -> anyhow::Result<String> {
    let content = files_from_path(path, recursive, optional)?
        .into_iter()
        .map(|file| read_file(&file))
        .collect::<anyhow::Result<Vec<_>>>()?
        .join("\n");
    Ok(content)
}

/// Reads the content from the given file or folder `path` and calls `parse`
/// for each non-empty line, with inline comments stripped.
///
/// The `path` can be a single file or a directory containing multiple files.
/// If the `path` is a directory and `recursive` is true, all files in the directory
/// are processed in filename order.
/// If `optional` is true, a nonexistent [`Path`] does not result in an `Err`.
pub fn parse_from_path<T>(
    path: &Path,
    recursive: bool,
    optional: bool,
    mut parse: impl FnMut(&str) -> anyhow::Result<T>,
) -> anyhow::Result<Vec<T>> {
    let mut values = Vec::new();
    for file in files_from_path(path, recursive, optional)? {
        let content = read_file(&file)?;
        values.extend(
            parse_lines(&content, &mut parse)
                .with_context(|| format!("failed to parse {}", file.display()))?,
        );
    }
    Ok(values)
}

/// Calls `parse` for each non-empty line in `content`, with inline comments stripped.
fn parse_lines<T>(
    content: &str,
    mut parse: impl FnMut(&str) -> anyhow::Result<T>,
) -> anyhow::Result<Vec<T>> {
    content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line = strip_line_comment(line);
            (!line.is_empty()).then_some((index + 1, line))
        })
        .map(|(lineno, line)| {
            parse(line).with_context(|| format!("error in line {lineno}: {line}"))
        })
        .collect()
}

/// Reads the given file `path` into a string.
fn read_file(path: &Path) -> anyhow::Result<String> {
    fs::read_to_string(path).with_context(|| format!("unable to read file {}", path.display()))
}

/// Collects the files that make up the given `path`.
///
/// A file path yields itself. A directory yields its files in filename order if `recursive` is
/// true.
/// If `optional` is true, a nonexistent [`Path`] does not result in an `Err`.
fn files_from_path(path: &Path, recursive: bool, optional: bool) -> anyhow::Result<Vec<PathBuf>> {
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(_) if optional => return Ok(Vec::new()),
        Err(e) => bail!("unable to access {}: {e}", path.display()),
    };
    if metadata.is_file() {
        return Ok(vec![path.to_owned()]);
    }
    if !recursive {
        bail!("{} is a directory, but should be a file", path.display());
    }

    utils::list_files(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_from_path() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let directory = temp.path().join("package.use");
        fs::create_dir(&directory)?;
        fs::write(directory.join("b"), "second # inline comment\n")?;
        fs::write(directory.join("a"), "# ignored\n\nfirst\n")?;

        let values = parse_from_path(&directory, true, true, |line| Ok(line.to_owned()))?;
        assert_eq!(values, ["first", "second"]);

        let missing = temp.path().join("missing");
        let values = parse_from_path::<String>(&missing, false, true, |_| unreachable!())?;
        assert!(values.is_empty());
        Ok(())
    }
}
