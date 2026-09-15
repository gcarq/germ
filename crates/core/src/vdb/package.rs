use crate::package::PackageView;
use crate::package::cpv::CPV;
use crate::package::metadata::{MetaVar, PackageMetadata};
use crate::repository::RepoName;
use crate::useflag::UseFlag;
use anyhow::Context;
use std::path::Path;
use std::str::FromStr;
use std::{fmt, fs, io};

/// Represents a package that is currently installed on the system.
pub struct InstalledPackage {
    cpv: CPV,
    repo: RepoName,
    metadata: PackageMetadata,
    useflags: Vec<UseFlag>,
}

impl InstalledPackage {
    /// Creates a new [`InstalledPackage`] from the given `CPV` and `path`.
    ///
    /// `path` is expected to be the VDB directory where additional metadata is stored.
    pub fn from_path(cpv: CPV, path: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            cpv,
            repo: fs::read_to_string(path.join("repository"))
                .context("unable to read repo")?
                .trim()
                .parse()?,
            metadata: load_metadata(path)?,
            useflags: fs::read_to_string(path.join("USE"))
                .context("unable to read USE flags")?
                .split_whitespace()
                .map(UseFlag::from_str)
                .collect::<anyhow::Result<Vec<_>>>()?,
        })
    }

    /// Creates a new [`InstalledPackage`] from the given `CPV`, `RepoName`, and `PackageMetadata`.
    pub const fn from_parts(
        cpv: CPV,
        repo: RepoName,
        metadata: PackageMetadata,
        useflags: Vec<UseFlag>,
    ) -> Self {
        Self {
            cpv,
            repo,
            metadata,
            useflags,
        }
    }

    /// Returns the enabled USE flags for this package.
    pub fn enabled_useflags(&self) -> &[UseFlag] {
        &self.useflags
    }
}

/// Loads the package metadata from the given package `path` in the VDB.
fn load_metadata(path: &Path) -> anyhow::Result<PackageMetadata> {
    let vars = MetaVar::ALL
        .iter()
        // `SRC_URI` is not stored in the VDB.
        .filter(|&var| *var != MetaVar::SrcUri)
        .map(|var| Ok((var.name(), read_meta(&path.join(var.name()))?)))
        .collect::<anyhow::Result<_>>()?;
    Ok(PackageMetadata::from_raw(&vars, None)?)
}

/// Reads the content of a metadata file at the given `path`.
fn read_meta(path: &Path) -> anyhow::Result<String> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => {
            Err(err).with_context(|| format!("unable to read metadata file {}", path.display()))
        }
    }
}

impl PackageView for InstalledPackage {
    fn cpv(&self) -> &CPV {
        &self.cpv
    }

    fn repo(&self) -> &RepoName {
        &self.repo
    }

    fn metadata(&self) -> &PackageMetadata {
        &self.metadata
    }
}

impl fmt::Display for InstalledPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.cpv, self.repo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{cpv, package_metadata};

    #[test]
    fn test_installed_package_fmt() {
        let cpv = cpv("app-editors", "vim", "7.0.174-r1");
        let pkg = InstalledPackage {
            cpv,
            repo: "gentoo".parse().unwrap(),
            metadata: package_metadata(&[]),
            useflags: Vec::new(),
        };
        assert_eq!(pkg.to_string(), "app-editors/vim-7.0.174-r1::gentoo");
    }
}
