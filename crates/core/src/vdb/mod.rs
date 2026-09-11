pub mod package;

use crate::deps::atom::Atom;
use crate::grammar::{PACKAGE, REVISION, VERSION, VERSION_SUFFIXES};
use crate::package::names::CatName;
use crate::package::version::PackageVersion;
use crate::package::{PackageView, cpv::CPV};
use crate::types::FxHashMap;
use crate::vdb::package::InstalledPackage;
use anyhow::{Context, anyhow};
use either::Either;
use fancy_regex::Regex;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Regex to validate and parse `package`, `version`, `suffixes` and the `revision` from VDB.
static VDB_PKG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"\A(?<package>{PACKAGE})-(?<version>{VERSION})(?<suffixes>{VERSION_SUFFIXES})(?:-r(?<revision>{REVISION}))?\z"
    ))
    .unwrap()
});

/// Takes care of handling the portage virtual database (VDB)
/// which contains [`InstalledPackage`].
pub struct Vdb {
    location: PathBuf,
    packages: FxHashMap<CatName, Vec<InstalledPackage>>,
    fully_loaded: bool,
}

impl Vdb {
    /// Creates a VDB from the given `location`.
    ///
    /// Returns `Err` if `location` doesn't exist or is not accessible.
    pub fn from_path(location: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let location = location.into();
        fs::metadata(&location)
            .with_context(|| format!("failed to access {}", location.display()))?;
        Ok(Self {
            location,
            packages: FxHashMap::default(),
            fully_loaded: false,
        })
    }

    /// Returns all packages matching the given `atom`.
    pub fn find_by_atom(&mut self, atom: &Atom) -> anyhow::Result<Vec<&InstalledPackage>> {
        match atom.category() {
            Some(cat) => {
                self.load_from_category(cat)
                    .with_context(|| format!("failed to load category {cat}"))?;
            }
            None if !self.fully_loaded => {
                self.load().context("failed to load packages from VDB")?;
            }
            None => {}
        }

        let iter = match atom.category() {
            Some(cat) => Either::Left(self.packages.get(cat).into_iter().flatten()),
            None => Either::Right(self.packages.values().flatten()),
        };
        Ok(iter
            .filter(|pkg| pkg.matches_atom(atom))
            .collect::<Vec<_>>())
    }

    /// Resolves all installed packages from the VDB root path.
    fn load(&mut self) -> anyhow::Result<()> {
        for entry in fs::read_dir(&self.location)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }

            let category = entry
                .file_name()
                .to_str()
                .ok_or_else(|| anyhow!("invalid path: {}", entry.path().display()))?
                .parse()?;
            self.load_from_category(&category)
                .with_context(|| format!("failed to load category {category}"))?;
        }
        self.fully_loaded = true;
        Ok(())
    }

    /// Resolves all installed packages for the given `category`.
    fn load_from_category(&mut self, category: &CatName) -> anyhow::Result<()> {
        if self.packages.contains_key(category) {
            return Ok(());
        }

        let path = self.location.join(category.as_str());
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                self.packages.insert(category.clone(), Vec::new());
                return Ok(());
            }
            Err(error) => Err(error)?,
        };

        let mut packages = entries
            .map(|entry| {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    return Ok(None);
                }
                Self::package_from_path(category, &entry.path())
            })
            .filter_map(Result::transpose)
            .collect::<anyhow::Result<Vec<_>>>()?;
        packages.sort_unstable_by(|a, b| b.cpv.cmp(&a.cpv));
        self.packages.insert(category.clone(), packages);
        Ok(())
    }

    /// Builds a [`Package`] from the given `path`.
    ///
    /// Returns `Ok(None)` if the path hasn't the correct syntax,
    ///   e.g. a package has an incomplete merge, indicated by the `-MERGING-` prefix.
    /// Returns `Err` if the path or metadata can't be read.
    fn package_from_path(
        category: &CatName,
        path: &Path,
    ) -> anyhow::Result<Option<InstalledPackage>> {
        let pvr = path
            .file_name()
            .and_then(|f| f.to_str())
            .with_context(|| format!("path contains invalid unicode: {}", path.display()))?;
        let Some(caps) = VDB_PKG_RE.captures(pvr)? else {
            return Ok(None);
        };

        let package = caps["package"].parse()?;
        let version = PackageVersion::new(
            &caps["version"],
            Some(&caps["suffixes"]),
            caps.name("revision").map(|m| m.as_str()),
        )?;
        let cpv = CPV::new(category.clone(), package, version);
        let pkg = InstalledPackage::new(cpv, path)
            .with_context(|| format!("failed to collect package from {}", path.display()))?;
        Ok(Some(pkg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::cpv;

    fn write_vdb_package(path: &Path, repository: &str) {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("repository"), repository).unwrap();
        fs::write(path.join("USE"), "").unwrap();
        fs::write(path.join("EAPI"), "8").unwrap();
        fs::write(path.join("DESCRIPTION"), "Test package").unwrap();
        fs::write(path.join("SLOT"), "0").unwrap();
    }

    #[test]
    fn test_vdb_from_path_missing() {
        let temp = tempfile::tempdir().unwrap();
        let result = Vdb::from_path(temp.path().join("missing"));
        assert!(result.is_err());
    }

    #[test]
    fn test_package_from_path() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("dev-libs").join("foo--1");
        write_vdb_package(&path, "repo-");

        let category: CatName = "dev-libs".parse().unwrap();
        let package = Vdb::package_from_path(&category, &path).unwrap().unwrap();
        assert_eq!(package.cpv, cpv("dev-libs", "foo-", "1"));
        assert_eq!(package.repo.as_str(), "repo-");

        let path = temp.path().join("dev-libs").join("foo-1");
        write_vdb_package(&path, "invalid name");
        assert!(Vdb::package_from_path(&category, &path).is_err());
    }

    #[test]
    fn test_find_by_atom() {
        let temp = tempfile::tempdir().unwrap();
        for path in [
            "dev-libs/bar-1",
            "dev-libs/bar-2",
            "dev-libs/foo-1",
            "app-editors/foo-3",
        ] {
            write_vdb_package(&temp.path().join(path), "gentoo");
        }

        let mut vdb = Vdb::from_path(temp.path().to_path_buf()).unwrap();

        let tests = [
            ("dev-libs/foo", vec!["dev-libs/foo-1"]),
            (
                "dev-libs/*",
                vec!["dev-libs/foo-1", "dev-libs/bar-2", "dev-libs/bar-1"],
            ),
            ("*/foo", vec!["dev-libs/foo-1", "app-editors/foo-3"]),
        ];
        for (atom, expected) in tests {
            let packages = vdb.find_by_atom(&Atom::new(atom).unwrap()).unwrap();
            let actual = packages
                .iter()
                .map(|package| package.cpv.fqn())
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn test_find_by_atom_missing_category() {
        let temp = tempfile::tempdir().unwrap();
        let mut vdb = Vdb::from_path(temp.path().to_path_buf()).unwrap();

        let packages = vdb
            .find_by_atom(&Atom::new("dev-libs/foo").unwrap())
            .unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn test_find_by_atom_invalid_metadata() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("dev-libs").join("foo-1")).unwrap();
        let mut vdb = Vdb::from_path(temp.path().to_path_buf()).unwrap();

        let result = vdb.find_by_atom(&Atom::new("dev-libs/foo").unwrap());
        assert!(result.is_err());
    }
}
