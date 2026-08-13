pub mod package;

use crate::deps::atom::Atom;
use crate::grammar::{PACKAGE, REVISION, VERSION, VERSION_SUFFIXES};
use crate::package::version::PackageVersion;
use crate::package::{PackageView, cpv::CPV};
use crate::vdb::package::InstalledPackage;
use anyhow::{Context, anyhow, bail};
use fancy_regex::Regex;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use walkdir::WalkDir;

/// Regex to validate and parse `package`, `version`, `suffixes` and the `revision` from VDB.
static VDB_PKG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"\A(?<package>{PACKAGE})-(?<version>{VERSION})(?<suffixes>{VERSION_SUFFIXES})(?:-r(?<revision>{REVISION}))?\z"
    ))
    .unwrap()
});

/// Represents a portage compatible VDB containing [`Package`].
pub struct Vdb {
    packages: Vec<InstalledPackage>,
}

impl Vdb {
    /// Collects and builds VDB from the given `path`.
    pub fn from_path(path: PathBuf) -> anyhow::Result<Self> {
        let packages = Self::collect_packages(&path)?;
        Ok(Self { packages })
    }

    /// Returns all packages matching the given `atom`.
    pub fn find_by_atom(&self, atom: &Atom) -> impl Iterator<Item = &InstalledPackage> {
        self.packages
            .iter()
            .filter(move |pkg| pkg.matches_atom(atom))
    }

    /// Collects all packages from the given VDB `path`.
    fn collect_packages(path: &Path) -> anyhow::Result<Vec<InstalledPackage>> {
        let paths = WalkDir::new(path)
            .min_depth(2)
            .max_depth(2)
            .into_iter()
            .filter_entry(|e| e.file_type().is_dir())
            .map(|entry| match entry {
                Ok(entry) => Ok(entry.into_path()),
                Err(e) => bail!("unable to read category in '{}': {e}", path.display()),
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let packages = paths
            .par_iter()
            .filter_map(|p| Self::package_from_path(p).transpose())
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(packages)
    }

    /// Builds a [`Package`] from the given `path`.
    ///
    /// Returns `Ok(None)` if the path hasn't the correct syntax,
    ///   e.g. a package has an incomplete merge, indicated by the `-MERGING-` prefix.
    /// Returns `Err` if the path or metadata can't be read.
    fn package_from_path(path: &Path) -> anyhow::Result<Option<InstalledPackage>> {
        let pvr = path
            .file_name()
            .and_then(|f| f.to_str())
            .with_context(|| "path contains invalid unicode")?;
        let Some(caps) = VDB_PKG_RE.captures(pvr)? else {
            return Ok(None);
        };

        let category = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|f| f.to_str())
            .with_context(|| "path contains invalid unicode")?
            .parse()
            .with_context(|| format!("unable to process VDB at {}", path.display()))?;
        let package = caps["package"].parse()?;
        let version = PackageVersion::new(
            &caps["version"],
            Some(&caps["suffixes"]),
            caps.name("revision").map(|m| m.as_str()),
        )?;
        let cpv = CPV::new(category, package, version);
        let pkg = InstalledPackage::new(cpv, path)
            .with_context(|| anyhow!("failed to collect package from {}", path.display()))?;
        Ok(Some(pkg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::cpv;
    use std::fs;

    fn write_vdb_package(path: &Path, repository: &str) {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("repository"), repository).unwrap();
        fs::write(path.join("USE"), "").unwrap();
        fs::write(path.join("EAPI"), "8").unwrap();
        fs::write(path.join("SLOT"), "0").unwrap();
    }

    #[test]
    fn test_package_from_path() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("dev-libs").join("foo--1");
        write_vdb_package(&path, "repo-");

        let package = Vdb::package_from_path(&path).unwrap().unwrap();
        assert_eq!(package.cpv, cpv("dev-libs", "foo-", "1"));
        assert_eq!(package.repo.as_str(), "repo-");

        for repository in ["", "repo-1", "invalid name"] {
            let path = temp.path().join("dev-libs").join("foo-1");
            write_vdb_package(&path, repository);

            assert!(Vdb::package_from_path(&path).is_err());
        }
    }
}
