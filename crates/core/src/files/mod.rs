//! This module contains common logic to handle file types often used in [`Repository`],
//! [`Profile`], etc.
//!
//! The most used one is [`LineBasedFile`], which is expressed as text file where each line holds
//! a value. [`LineBasedFile`] can be inherited and any value prefixed with a hyphen negates
//! the same previous defined item, e.g.: `package.use`, `use.mask`, etc.
pub mod entry;
mod linefile;
pub mod pkguse;

use crate::deps::atom::Atom;
use crate::deps::useflag::UseFlag;
use crate::files::entry::SysAtom;
use crate::utils;
use anyhow::{Context, Result, anyhow, bail};
use linefile::LineBasedFile;
use std::fs;
use std::path::Path;

pub type PackageEntries = LineBasedFile<Atom>;
pub type SysPackageEntries = LineBasedFile<SysAtom>;
pub type UseEntries = LineBasedFile<UseFlag>;

/// Reads the content from the given file or folder `path`.
///
/// The `path` can point to a single file or a directory containing multiple files.
/// If the `path` is a directory and `recursive` is true, all files in the directory
/// are concatenated together in order of their filename.
/// If `optional` is true, the absence of the [`Path`] does not result in an `Err`.
pub fn content_from_path(path: &Path, recursive: bool, optional: bool) -> Result<String> {
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

    let content = utils::list_files(path)
        .map(|p| match p {
            Ok(p) => fs::read_to_string(&p)
                .with_context(|| anyhow!("unable to read file '{}'", p.display())),
            Err(err) => bail!(err),
        })
        .collect::<Result<Vec<_>>>()?
        .join("\n");
    Ok(content)
}
