use crate::package::PackageView;
use crate::package::cpv::CPV;
use crate::package::metadata::PackageMetadata;
use crate::repository::RepoName;
use crate::useflag::UseFlag;
use anyhow::Context;
use std::path::Path;
use std::str::FromStr;
use std::{fmt, fs};

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
        let repo = fs::read_to_string(path.join("repository"))
            .context("unable to read repo")?
            .trim()
            .parse()?;
        let useflags = fs::read_to_string(path.join("USE"))
            .context("unable to read USE flags")?
            .split_whitespace()
            .map(UseFlag::from_str)
            .collect::<anyhow::Result<Vec<_>>>()?;

        let metadata = PackageMetadata::from_vdb_path(path).context("unable to read metadata")?;
        Ok(Self {
            cpv,
            repo,
            metadata,
            useflags,
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
    use crate::test_support::cpv;

    #[test]
    fn test_installed_package_fmt() {
        let cpv = cpv("app-editors", "vim", "7.0.174-r1");
        let pkg = InstalledPackage {
            cpv,
            repo: "gentoo".parse().unwrap(),
            metadata: PackageMetadata::default(),
            useflags: Vec::new(),
        };
        assert_eq!(pkg.to_string(), "app-editors/vim-7.0.174-r1::gentoo");
    }
}
