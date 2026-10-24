pub mod package;
#[cfg(test)]
pub(crate) mod test_support;

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use either::Either;
use germ_pms::package::parse_pf;
use germ_pms::{Atom, CatName, Cpv, PackageView};

use crate::types::FxHashMap;
use crate::vdb::package::InstalledPackage;

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
        let pf = path
            .file_name()
            .and_then(|f| f.to_str())
            .with_context(|| format!("path contains invalid unicode: {}", path.display()))?;
        let Some((package, version)) = parse_pf(pf) else {
            return Ok(None);
        };
        let cpv = Cpv::new(category.clone(), package, version);
        let pkg = InstalledPackage::from_path(cpv, path)
            .with_context(|| format!("failed to collect package from {}", path.display()))?;
        Ok(Some(pkg))
    }
}

#[cfg(test)]
mod tests {
    use germ_pms::test_support::{cpv, pkg};

    use super::test_support::VdbFixture;
    use super::*;

    #[test]
    fn test_vdb_from_path_missing() {
        let temp = tempfile::tempdir().unwrap();
        let result = Vdb::from_path(temp.path().join("missing"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_package() -> anyhow::Result<()> {
        let fixture = VdbFixture::new()?;

        fixture.package("dev-libs/foo--1").repo("repo-").write()?;
        let mut vdb = fixture.vdb()?;
        let atom = Atom::new("dev-libs/foo-")?;

        let pkg = vdb.find_by_atom(&atom)?.next().unwrap();
        assert_eq!(pkg.cpv(), &cpv("dev-libs/foo--1"));
        assert_eq!(pkg.repo().as_str(), "repo-");
        Ok(())
    }

    #[test]
    fn test_load_package_invalid_repository() -> anyhow::Result<()> {
        let fixture = VdbFixture::new()?;
        fixture
            .package("sys-libs/pam-1")
            .repo("invalid name")
            .write()?;
        let mut vdb = fixture.vdb()?;
        let atom = Atom::new("sys-libs/pam")?;

        assert!(vdb.find_by_atom(&atom).is_err());
        Ok(())
    }

    #[test]
    fn test_find_by_atom() -> anyhow::Result<()> {
        let fixture = VdbFixture::new()?;
        for fqn in [
            "sys-libs/zlib-1",
            "sys-libs/zlib-2",
            "sys-libs/pam-1",
            "app-alternatives/tar-1",
            "app-arch/tar-1",
        ] {
            fixture.package(fqn).write()?;
        }
        let mut vdb = fixture.vdb()?;

        let tests = [
            ("sys-libs/pam", vec!["sys-libs/pam-1"]),
            (
                "sys-libs/*",
                vec!["sys-libs/pam-1", "sys-libs/zlib-1", "sys-libs/zlib-2"],
            ),
            ("*/tar", vec!["app-alternatives/tar-1", "app-arch/tar-1"]),
        ];
        for (atom, expected) in tests {
            let atom = Atom::new(atom)?;
            let pkgs = vdb.find_by_atom(&atom)?;
            let mut actual = pkgs.map(|pkg| pkg.cpv().fqn()).collect::<Vec<_>>();
            actual.sort_unstable();
            assert_eq!(actual, expected);
        }
        Ok(())
    }

    #[test]
    fn test_find_by_pkg() -> anyhow::Result<()> {
        let fixture = VdbFixture::new()?;
        fixture.package("sys-libs/pam-1").slot("0").write()?;
        fixture.package("sys-libs/pam-2").slot("1").write()?;
        let mut vdb = fixture.vdb()?;

        let same_slot = pkg("sys-libs/pam-2", &[("SLOT", "0")]);
        let diff_slot = pkg("sys-libs/pam-2", &[("SLOT", "2")]);

        let result = vdb.find_by_pkg(&same_slot)?.map(PackageView::cpv);
        assert_eq!(result, Some(&cpv("sys-libs/pam-1")));
        assert!(vdb.find_by_pkg(&diff_slot)?.is_none());
        Ok(())
    }

    #[test]
    fn test_package_from_path_invalid() {
        let category = "sys-libs".parse().unwrap();
        let result = Vdb::package_from_path(&category, Path::new("pam-1-2"));
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_find_by_atom_missing_category() -> anyhow::Result<()> {
        let fixture = VdbFixture::new()?;
        let mut vdb = fixture.vdb()?;
        let atom = Atom::new("sys-apps/grep")?;

        assert!(vdb.find_by_atom(&atom)?.next().is_none());
        Ok(())
    }
}
