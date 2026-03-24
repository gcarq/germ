//! This module contains common logic to handle file types often used in [`Repository`],
//! [`Profile`], etc.
//!
//! The most used one is [`LineBasedFile`], which is expressed as text file where each line holds
//! a value. [`LineBasedFile`] can be inherited and any value prefixed with a hyphen negates
//! the same previous defined item, e.g.: `package.use`, `use.mask`, etc.
mod entry;
mod linefile;
pub mod pkguse;

use crate::deps::atom::Atom;
use crate::deps::useflag::UseFlag;
use crate::files::entry::SysAtom;
use crate::utils;
use anyhow::{Context, anyhow};
use linefile::LineBasedFile;
use std::fs;
use std::path::Path;

pub type PackageEntries = LineBasedFile<Atom>;
pub type SysPackageEntries = LineBasedFile<SysAtom>;
pub type UseEntries = LineBasedFile<UseFlag>;

/// Trait for types that can be constructed from file(s) at a given path.
pub trait FileFromPath {
    /// Creates an instance from the file(s) at the given `path`.
    /// The `path` can point to a single file or a directory containing multiple files.
    /// If the `path` is a directory and `recursive` is true, all files in the directory
    /// are concatenated together in order of their filename.
    /// If `optional` is true, the absence of the [`Path`] does not result in an `Err`.
    fn from_path(path: &Path, recursive: bool, optional: bool) -> anyhow::Result<Self>
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
            return Self::from_string(content)
                .with_context(|| anyhow!("error while processing file {}", path.display()));
        }

        if !recursive {
            return Err(anyhow!(
                "{} is a directory, but should be a file",
                path.display()
            ));
        }

        let content = utils::list_files(path)
            .map(|p| match p {
                Ok(p) => fs::read_to_string(&p)
                    .with_context(|| anyhow!("unable to read file '{}'", p.display())),
                Err(err) => Err(anyhow!(err)),
            })
            .collect::<anyhow::Result<Vec<_>>>()?
            .join("\n");
        Self::from_string(content)
            .with_context(|| anyhow!("error while processing directory: {}", path.display()))
    }

    /// Creates an instance from the `content`.
    fn from_string(content: String) -> anyhow::Result<Self>
    where
        Self: Sized;
}
