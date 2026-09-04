//! This module contains common logic to handle file types often used in [`Repository`],
//! [`Profile`], etc.
//!
//! The most used one is [`LineEntries`], which is expressed as text file where each line holds
//! a value. [`LineEntries`] can be inherited and any value prefixed with a hyphen negates
//! the same previous defined item, e.g.: `package.use`, `use.mask`, etc.
pub mod entry;
mod linefile;
pub mod pkgfile;

use crate::deps::atom::Atom;
use crate::files::entry::SysAtom;
use crate::useflag::UseFlag;
use crate::utils;
use anyhow::{Context, anyhow, bail};
use linefile::LineEntries;
use std::fs;
use std::path::Path;

pub type PackageEntries = LineEntries<Atom>;
pub type SysPackageEntries = LineEntries<SysAtom>;
pub type UseEntries = LineEntries<UseFlag>;

/// Reads the content from the given file or folder `path`.
///
/// The `path` can point to a single file or a directory containing multiple files.
/// If the `path` is a directory and `recursive` is true, all files in the directory
/// are concatenated together in order of their filename.
/// If `optional` is true, the absence of the [`Path`] does not result in an `Err`.
pub fn content_from_path(path: &Path, recursive: bool, optional: bool) -> anyhow::Result<String> {
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(_) if optional => return Ok(String::default()),
        Err(e) => bail!("unable to access {}: {e}", path.display()),
    };
    if metadata.is_file() {
        return fs::read_to_string(path)
            .with_context(|| anyhow!("error while processing file {}", path.display()));
    }

    if !recursive {
        bail!("{} is a directory, but should be a file", path.display());
    }

    let content = utils::list_files(path)?
        .into_iter()
        .map(|path| {
            fs::read_to_string(&path)
                .with_context(|| anyhow!("unable to read file '{}'", path.display()))
        })
        .collect::<anyhow::Result<Vec<_>>>()?
        .join("\n");
    Ok(content)
}
