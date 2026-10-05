use std::cmp::Ordering;
use std::path::Path;
use std::str::FromStr;
use std::{fmt, fs, hash, io};

use anyhow::{Context, bail};
use germ_pms::{CPV, MetaVar, PackageMetadata, RepoName, UseFlag};

use crate::eapi::is_supported_for_ebuilds;
use crate::package::PackageView;
use crate::useflag::EffectiveUse;

/// Represents a package that is currently installed on the system.
#[derive(Debug, Clone)]
pub struct InstalledPackage {
    cpv: CPV,
    repo: RepoName,
    metadata: PackageMetadata,
    iuse_effective: EffectiveUse,
}

impl InstalledPackage {
    /// Creates a new [`InstalledPackage`] from the given `CPV` and `path`.
    ///
    /// `path` is expected to be the VDB directory where additional metadata is stored.
    pub fn from_path(cpv: CPV, path: &Path) -> anyhow::Result<Self> {
        let iuse_effective = EffectiveUse::from_parts(
            read_meta(&path.join("IUSE_EFFECTIVE"))?
                .split_whitespace()
                .map(UseFlag::from_str)
                .collect::<anyhow::Result<_>>()?,
            fs::read_to_string(path.join("USE"))
                .context("unable to read USE flags")?
                .split_whitespace()
                .map(UseFlag::from_str)
                .collect::<anyhow::Result<_>>()?,
        );
        Ok(Self {
            cpv,
            repo: fs::read_to_string(path.join("repository"))
                .context("unable to read repo")?
                .trim()
                .parse()?,
            metadata: load_metadata(path)?,
            iuse_effective,
        })
    }

    /// Creates a new [`InstalledPackage`] from the given parts.
    pub const fn from_parts(
        cpv: CPV,
        repo: RepoName,
        metadata: PackageMetadata,
        iuse_effective: EffectiveUse,
    ) -> Self {
        Self {
            cpv,
            repo,
            metadata,
            iuse_effective,
        }
    }

    /// Returns the effective USE state when the package was installed.
    pub const fn effective_use(&self) -> &EffectiveUse {
        &self.iuse_effective
    }
}

impl Eq for InstalledPackage {}

impl PartialEq for InstalledPackage {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Ord for InstalledPackage {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cpv.cmp(&other.cpv).then(self.repo.cmp(&other.repo))
    }
}

impl PartialOrd for InstalledPackage {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for InstalledPackage {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.cpv.hash(state);
        self.repo.hash(state);
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
    let metadata = PackageMetadata::from_raw(&vars, None)?;
    if !is_supported_for_ebuilds(&metadata.eapi()) {
        bail!("EAPI {} is not supported for ebuilds", metadata.eapi());
    }
    Ok(metadata)
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
    use crate::test_support::{cpv, pkg_metadata};
    use crate::useflag::test_support::effective;

    #[test]
    fn test_installed_package_fmt() {
        let pkg = InstalledPackage::from_parts(
            cpv("app-editors", "vim", "7.0.174-r1"),
            "gentoo".parse().unwrap(),
            pkg_metadata(&[]),
            EffectiveUse::default(),
        );
        assert_eq!(pkg.to_string(), "app-editors/vim-7.0.174-r1::gentoo");
    }

    #[test]
    fn test_load_metadata_unsupported_eapi() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("EAPI"), "6").unwrap();
        fs::write(directory.path().join("DESCRIPTION"), "Test package").unwrap();
        fs::write(directory.path().join("SLOT"), "0").unwrap();

        assert!(load_metadata(directory.path()).is_err());
    }

    #[test]
    fn test_effective_use() {
        let flag: UseFlag = "flag".parse().unwrap();
        let pkg = InstalledPackage::from_parts(
            cpv("app-misc", "foo", "1"),
            "gentoo".parse().unwrap(),
            pkg_metadata(&[]),
            effective(&["flag"], &[]),
        );

        assert_eq!(pkg.effective_use().state(&flag), Some(false));
    }
}
