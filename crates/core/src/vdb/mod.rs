pub mod package;
#[cfg(test)]
pub(crate) mod test_support;

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context, anyhow};
use either::Either;
use fancy_regex::Regex;
use germ_pms::grammar::{PACKAGE, REVISION, VERSION, VERSION_SUFFIXES};
use germ_pms::{Atom, CPV, CatName, PackageVersion};

use crate::package::PackageView;
use crate::types::FxHashMap;
use crate::vdb::package::InstalledPackage;

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

    /// Returns `true` if a package that matches `atom` is installed.
    pub fn is_installed(&mut self, atom: &Atom) -> anyhow::Result<bool> {
        self.find_by_atom(atom)
            .map(|mut iter| iter.next().is_some())
    }

    /// Returns all packages matching the given `atom`.
    pub fn find_by_atom<'a, 'atom>(
        &'a mut self,
        atom: &'atom Atom,
    ) -> anyhow::Result<impl Iterator<Item = &'a InstalledPackage> + use<'a, 'atom>> {
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
        Ok(iter.filter(|pkg| pkg.matches_atom(atom)))
    }

    /// Returns an [`InstalledPackage`] that matches the given `pkg`,
    /// based on the category and package name and whether they are in the same slot.
    pub fn find_by_pkg<'a, P: PackageView>(
        &'a mut self,
        pkg: &P,
    ) -> anyhow::Result<Option<&'a InstalledPackage>> {
        let category = pkg.category();
        self.load_from_category(category)
            .with_context(|| format!("failed to load category {category}"))?;
        Ok(self
            .packages
            .get(category)
            .into_iter()
            .flatten()
            .find(|p| p.matches_package_slot(pkg)))
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
        packages.sort_unstable_by(|a, b| b.cpv().cmp(a.cpv()));
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
        let pkg = InstalledPackage::from_path(cpv, path)
            .with_context(|| format!("failed to collect package from {}", path.display()))?;
        Ok(Some(pkg))
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::VdbFixture;
    use super::*;
    use crate::test_support::{cpv, pkg};

    #[test]
    fn test_vdb_from_path_missing() {
        let temp = tempfile::tempdir().unwrap();
        let result = Vdb::from_path(temp.path().join("missing"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_package() -> anyhow::Result<()> {
        let fixture = VdbFixture::new()?;
        fixture
            .package("dev-libs", "foo-", "1")
            .repo("repo-")
            .write()?;
        let mut vdb = fixture.vdb()?;
        let atom = Atom::new("dev-libs/foo-")?;

        let pkg = vdb.find_by_atom(&atom)?.next().unwrap();
        assert_eq!(pkg.cpv(), &cpv("dev-libs", "foo-", "1"));
        assert_eq!(pkg.repo().as_str(), "repo-");
        Ok(())
    }

    #[test]
    fn test_load_package_invalid_repository() -> anyhow::Result<()> {
        let fixture = VdbFixture::new()?;
        fixture
            .package("dev-libs", "foo", "1")
            .repo("invalid name")
            .write()?;
        let mut vdb = fixture.vdb()?;
        let atom = Atom::new("dev-libs/foo")?;

        assert!(vdb.find_by_atom(&atom).is_err());
        Ok(())
    }

    #[test]
    fn test_find_by_atom() -> anyhow::Result<()> {
        let fixture = VdbFixture::new()?;
        for (category, package, version) in [
            ("dev-libs", "bar", "1"),
            ("dev-libs", "bar", "2"),
            ("dev-libs", "foo", "1"),
            ("app-editors", "foo", "3"),
        ] {
            fixture.package(category, package, version).write()?;
        }
        let mut vdb = fixture.vdb()?;

        let tests = [
            ("dev-libs/foo", vec!["dev-libs/foo-1"]),
            (
                "dev-libs/*",
                vec!["dev-libs/foo-1", "dev-libs/bar-2", "dev-libs/bar-1"],
            ),
            ("*/foo", vec!["dev-libs/foo-1", "app-editors/foo-3"]),
        ];
        for (atom, expected) in tests {
            let atom = Atom::new(atom)?;
            let pkgs = vdb.find_by_atom(&atom)?;
            let actual = pkgs.map(|pkg| pkg.cpv().fqn()).collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
        Ok(())
    }

    #[test]
    fn test_find_by_pkg() -> anyhow::Result<()> {
        let fixture = VdbFixture::new()?;
        fixture.package("app-misc", "foo", "1").slot("0").write()?;
        fixture.package("app-misc", "foo", "2").slot("1").write()?;
        let mut vdb = fixture.vdb()?;

        let same_slot = pkg("app-misc", "foo", "3", &[("SLOT", "0")]);
        let diff_slot = pkg("app-misc", "foo", "3", &[("SLOT", "2")]);

        let result = vdb.find_by_pkg(&same_slot)?.map(PackageView::cpv);
        assert_eq!(result, Some(&cpv("app-misc", "foo", "1")));
        assert!(vdb.find_by_pkg(&diff_slot)?.is_none());
        Ok(())
    }

    #[test]
    fn test_find_by_atom_missing_category() -> anyhow::Result<()> {
        let fixture = VdbFixture::new()?;
        let mut vdb = fixture.vdb()?;
        let atom = Atom::new("dev-libs/foo")?;

        assert!(vdb.find_by_atom(&atom)?.next().is_none());
        Ok(())
    }
}
