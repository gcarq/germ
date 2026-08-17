use crate::package::PackageView;
use crate::package::cpv::CPV;
use crate::package::metadata::PackageMetadata;
use crate::package::slot::PackageSlot;
use crate::repository::RepoName;
use crate::useflag::UseFlag;
use anyhow::Context;
use std::path::Path;
use std::str::FromStr;
use std::{fmt, fs};

pub struct InstalledPackage {
    pub cpv: CPV,
    pub repo: RepoName,
    pub metadata: PackageMetadata,
    pub use_flags: Vec<UseFlag>,
}

impl InstalledPackage {
    /// Creates a new [`InstalledPackage`] from the given `CPV`.
    ///
    /// `path` is the path to the packages vdb directory where additional metadata can be queried.
    pub fn new(cpv: CPV, path: &Path) -> anyhow::Result<Self> {
        let repo = fs::read_to_string(path.join("repository"))
            .with_context(|| "unable to read repo")?
            .trim()
            .parse()?;
        let use_flags = fs::read_to_string(path.join("USE"))
            .with_context(|| "unable to read USE flags")?
            .split_whitespace()
            .map(UseFlag::from_str)
            .collect::<anyhow::Result<Vec<_>>>()?;

        let metadata =
            PackageMetadata::from_vdb_path(path).with_context(|| "unable to read metadata")?;
        Ok(Self {
            cpv,
            repo,
            metadata,
            use_flags,
        })
    }
}

impl PackageView for InstalledPackage {
    fn cpv(&self) -> &CPV {
        &self.cpv
    }

    fn repo(&self) -> &RepoName {
        &self.repo
    }

    fn slot(&self) -> &PackageSlot {
        &self.metadata.slot
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
            use_flags: Vec::new(),
        };
        assert_eq!(pkg.to_string(), "app-editors/vim-7.0.174-r1::gentoo");
    }
}
