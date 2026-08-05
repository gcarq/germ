mod eclass;
mod error;
mod layout;
mod package;
mod profiles;

pub use eclass::{Eclass, Eclasses};
pub use error::RepositoryError;
pub use layout::{Layout, LayoutError};
pub use package::PackageResolutionError;
pub use profiles::{ArchList, ProfileError};

use crate::consts::DEFAULT_CACHE_PATH;
use crate::deps::atom::Atom;
use crate::eapi::Eapi;
use crate::files::{PackageEntries, entry::Precedence};
use crate::package::{Package, cpv::CPV};
use crate::regex::REPO_RE;
use crate::types::FxHashSet;
use crate::utils::Inherit;

use self::package::{
    AvailablePackageIndex, IndexError, ResolvedPackageIndex, resolve_cpv_from_category,
};
use self::profiles::ProfileDescriptions;
use anyhow::{Context, anyhow};
use log::{debug, warn};
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::{fmt, fs};

/// Represents an available ebuild repository.
/// See https://projects.gentoo.org/pms/8/pms.html#x1-290004.1
#[derive(Debug)]
#[cfg_attr(test, derive(Default))]
pub struct Repository {
    pub location: PathBuf,
    pub name: String,
    pub(crate) layout: Layout,
    pub(crate) package_mask: PackageEntries,
    pub(crate) package_unmask: PackageEntries,
    pub(crate) eclasses: Eclasses,
    pub(crate) arch_list: ArchList,
    pub(crate) categories: FxHashSet<String>,
    profiles_desc: ProfileDescriptions,
    avail_package_idx: AvailablePackageIndex,
    resolved_package_idx: ResolvedPackageIndex,
}

impl Repository {
    /// Loads intrinsic repository data from disk.
    pub(super) fn load(name: &str, location: &Path) -> Result<Self, RepositoryError> {
        let layout = Layout::from_path(&location.join("metadata").join("layout.conf"))?;
        let profiles = location.join("profiles");
        let eapi = Eapi::from_eapi_file(&profiles.join("eapi")).map_err(ProfileError::from)?;

        let dir_support = eapi.supports_profile_file_dirs() || layout.supports_profile_file_dirs();
        let package_mask = PackageEntries::from_path(
            &profiles.join("package.mask"),
            Precedence::Repository,
            dir_support,
        )
        .map_err(|err| ProfileError::from(err.context("unable to load package.mask")))?;
        let package_unmask = PackageEntries::from_path(
            &profiles.join("package.unmask"),
            Precedence::Repository,
            dir_support,
        )
        .map_err(|err| ProfileError::from(err.context("unable to load package.unmask")))?;

        Ok(Self {
            location: location.to_owned(),
            name: Self::resolve_repo_name(name, &layout, &profiles)?.to_owned(),
            layout,
            categories: FxHashSet::default(),
            package_mask,
            package_unmask,
            eclasses: Eclasses::empty(location),
            arch_list: ArchList::from_path(&profiles.join("arch.list"))?,
            profiles_desc: ProfileDescriptions::from_path(&profiles.join("profiles.desc"))?,
            avail_package_idx: AvailablePackageIndex::default(),
            resolved_package_idx: ResolvedPackageIndex::default(),
        })
    }

    /// Returns all known CPVs in the repository.
    pub fn cpvs(&self) -> impl Iterator<Item = &CPV> {
        self.avail_package_idx.values().flatten()
    }

    /// Returns all known packages (including metadata) in the repository.
    ///
    /// This function will call [`Repository::build_package_index()`] and
    /// index all missing packages.
    pub fn packages(&mut self) -> impl Iterator<Item = Result<&Package, PackageResolutionError>> {
        let errors = self.build_package_index();
        let packages = self.resolved_package_idx.values().map(Ok);
        packages.chain(errors.into_iter().map(Err))
    }

    /// Finds and returns all packages that match the given [`Atom`].
    /// TODO: optimize this function
    pub fn find_packages(
        &mut self,
        atom: &Atom,
    ) -> impl Iterator<Item = Result<&Package, PackageResolutionError>> {
        let cpvs = self
            .avail_package_idx
            .find_packages(atom)
            .into_iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();

        let errors = self.build_index_for_cpvs(&cpvs);
        cpvs.into_iter()
            .filter_map(|cpv| self.resolved_package_idx.get(cpv.fqn()).map(Ok))
            .chain(errors.into_iter().map(Err))
    }

    /// Builds the [`Package`] index for all known [`CPV`].
    pub fn build_package_index(&mut self) -> Vec<PackageResolutionError> {
        let cpvs = self
            .avail_package_idx
            .values()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        self.build_index_for_cpvs(&cpvs)
    }

    /// Checks if the profile with the relative `profile_path` is valid for the given `arch`.
    ///
    /// The repository location prefix must be stripped from the passed `profile_path` string
    /// e.g.: `default/linux/23.0`
    pub fn is_known_profile(&self, arch: &str, profile_path: &str) -> bool {
        self.profiles_desc
            .iter()
            .any(|desc| desc.keyword == arch && desc.profile_path == profile_path)
    }

    /// Writes [`ResolvedPackageIndex`] to disk.
    ///
    /// If `force` is `true`, the index will be written even if it hasn't been modified
    /// since the last write.
    ///
    /// Returns `Err` if the index cannot be serialized or the file cannot be created.
    pub fn write_index(&self, force: bool) -> Result<(), RepositoryError> {
        let meta_dir = PathBuf::from(DEFAULT_CACHE_PATH).join("metadata");
        fs::create_dir_all(&meta_dir).map_err(|error| {
            RepositoryError::Data(anyhow!(error).context(format!(
                "unable to create package index directory at {}",
                meta_dir.display()
            )))
        })?;
        debug!(
            "Writing package index for '{self}' into {} ...",
            meta_dir.display()
        );
        let path = meta_dir.join(&self.name);
        self.write_index_to_path(&path, force)
    }

    /// Builds the [`Package`] index for the given `cpvs`.
    fn build_index_for_cpvs(&mut self, cpvs: &[CPV]) -> Vec<PackageResolutionError> {
        let outcomes = cpvs
            .into_par_iter()
            .filter(|cpv| !self.resolved_package_idx.contains_key(cpv.fqn()))
            .map(|cpv| self.resolve_package(cpv))
            .collect::<Vec<_>>();

        outcomes
            .into_iter()
            .filter_map(|outcome| match outcome {
                Ok(pkg) => {
                    self.resolved_package_idx.insert(pkg);
                    None
                }
                Err(error) => Some(error),
            })
            .collect()
    }

    fn write_index_to_path(&self, path: &Path, force: bool) -> Result<(), RepositoryError> {
        self.resolved_package_idx
            .write_to_path(path, force)
            .map_err(|error| match error {
                error @ IndexError::Io(_) => RepositoryError::Data(anyhow!(error).context(
                    format!("unable to write package index at {}", path.display()),
                )),
                error @ IndexError::Serialization(_) => RepositoryError::Internal(
                    anyhow!(error).context("unable to serialize package index"),
                ),
            })
    }

    /// Resolves the given [`CPV`] into a [`Package`] with its metadata.
    fn resolve_package(&self, cpv: &CPV) -> Result<Package, PackageResolutionError> {
        let ebuild_path = self
            .location
            .join(cpv.category())
            .join(cpv.package())
            .join(format!("{}.ebuild", cpv.pf()));
        let metadata = cpv
            .generate_metadata(&ebuild_path, self)
            .map_err(|error| PackageResolutionError::from_metadata(cpv, error))?;
        Ok(Package::new(cpv.clone(), self.name.clone(), metadata))
    }

    /// Loads the resolved package index from disk.
    ///
    /// Returns `Err` if the index file exists but cannot be opened.
    pub(crate) fn load_index(&mut self) -> Result<(), RepositoryError> {
        let path = PathBuf::from(DEFAULT_CACHE_PATH)
            .join("metadata")
            .join(&self.name);
        self.load_index_from_path(&path)
    }

    fn load_index_from_path(&mut self, path: &Path) -> Result<(), RepositoryError> {
        debug!(
            "Loading package index for '{self}' from {} ...",
            path.display()
        );
        if let Some(index) = ResolvedPackageIndex::load_from_path(path).map_err(|error| {
            RepositoryError::Data(anyhow!(error).context(format!(
                "unable to load package index at {}",
                path.display()
            )))
        })? {
            self.resolved_package_idx = index;
            self.resolved_package_idx.retain(&self.avail_package_idx);
        }
        Ok(())
    }

    /// Populates all categories, packages and eclasses.
    ///
    /// NOTE: The caller must ensure [`Inherit::inherit_from`] has been called before.
    pub(crate) fn populate(&mut self) -> Result<(), RepositoryError> {
        self.collect_eclasses().map_err(RepositoryError::Data)?;
        self.collect_categories();
        self.collect_cpvs().map_err(RepositoryError::Data)?;
        Ok(())
    }

    /// Collects all known eclasses in the repo.
    fn collect_eclasses(&mut self) -> anyhow::Result<()> {
        let path = self.location.join("eclass");
        let eclasses = Eclasses::from_path(&path)
            .with_context(|| format!("unable to collect eclasses at {}", path.display()))?;
        self.eclasses.extend(&eclasses);
        Ok(())
    }

    /// Collects all known categories in the repository.
    fn collect_categories(&mut self) {
        let path = self.location.join("profiles").join("categories");
        if let Ok(content) = fs::read_to_string(&path) {
            self.categories
                .extend(content.lines().map(ToOwned::to_owned));
        }
    }

    /// Collects all known packages in the repository as [`CPV`].
    ///
    /// NOTE: The caller must ensure to [`Self::collect_categories`] has been called before,
    /// since only known categories are considered when collecting packages.
    fn collect_cpvs(&mut self) -> anyhow::Result<()> {
        let packages = self
            .categories
            .par_iter()
            .flat_map_iter(|category| resolve_cpv_from_category(&self.location, category))
            .collect::<anyhow::Result<Vec<_>>>()
            .with_context(|| {
                format!("unable to collect packages at {}", self.location.display())
            })?;
        self.avail_package_idx.insert_all(packages);
        Ok(())
    }

    /// Resolves the repo name and validates it against `profiles/repo_name` and `layout.conf`.
    ///
    /// The given `name` should be the name of the repository as defined in `repos.conf`.
    fn resolve_repo_name<'a>(
        name: &'a str,
        layout: &Layout,
        profiles: &Path,
    ) -> Result<&'a str, ProfileError> {
        let declared_name = if let Some(declared_name) = &layout.name {
            declared_name.clone()
        } else {
            let path = profiles.join("repo_name");
            fs::read_to_string(&path)
                .map_err(|err| {
                    anyhow!(err)
                        .context(anyhow!("unable to read repo_name from {}", path.display()))
                })?
                .lines()
                .next()
                .ok_or_else(|| ProfileError::from(anyhow!("empty repo_name file")))
                .map(ToOwned::to_owned)?
        };
        if declared_name != name {
            warn!(
                "Repository name mismatch: repo_name='{declared_name}' vs repos.conf='{name}'! Using {name}..."
            );
        }
        if !REPO_RE.is_match(name) {
            return Err(anyhow!("invalid repository name: '{name}'").into());
        }
        Ok(name)
    }
}

impl Inherit for Repository {
    /// Inherits relevant metadata from the given `master` repository.
    fn inherit_from(&mut self, master: &Repository) {
        debug!("Inheriting '{}' from '{}' ...", self.name, master.name);
        self.categories.extend(master.categories.iter().cloned());
        self.eclasses.extend(&master.eclasses);
    }
}

impl Eq for Repository {}

impl PartialEq for Repository {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Hash for Repository {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl fmt::Display for Repository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::test_support::RepositoryFixture;
    use crate::package::version::PackageVersion;

    use tempfile::tempdir;

    #[test]
    fn test_invalid_package_versions() {
        let location = RepositoryFixture::new("repo")
            .categories(["app-misc"])
            .ebuild("app-misc", "foo", "1")
            .ebuild("app-misc", "foo", "2")
            .write()
            .unwrap();
        let mut repository = Repository::load("repo", &location).unwrap();
        repository.populate().unwrap();

        let results = repository
            .find_packages(&Atom::new("app-misc/foo").unwrap())
            .collect::<Vec<_>>();

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(Result::is_err));
    }

    #[test]
    fn test_metadata_failure() {
        let location = RepositoryFixture::new("repo")
            .categories(["app-misc"])
            .ebuild("app-misc", "foo", "1")
            .ebuild("app-misc", "foo", "2")
            .write()
            .unwrap();
        let mut repository = Repository::load("repo", &location).unwrap();
        repository.populate().unwrap();
        let valid_cpv =
            CPV::new("app-misc", "foo", PackageVersion::try_from("1").unwrap()).unwrap();
        repository.resolved_package_idx.insert(Package::new(
            valid_cpv,
            "repo".into(),
            Default::default(),
        ));

        let results = repository
            .find_packages(&Atom::new("app-misc/foo").unwrap())
            .collect::<Vec<_>>();

        assert_eq!(results.len(), 2);
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    }

    #[test]
    fn test_missing_package_index() {
        let location = RepositoryFixture::new("repo").write().unwrap();
        let mut repository = Repository::load("repo", &location).unwrap();
        let directory = tempdir().unwrap();

        repository
            .load_index_from_path(&directory.path().join("missing"))
            .unwrap();
    }

    #[test]
    fn test_corrupt_package_index() {
        let location = RepositoryFixture::new("repo").write().unwrap();
        let mut repository = Repository::load("repo", &location).unwrap();
        let directory = tempdir().unwrap();
        let path = directory.path().join("index");
        fs::write(&path, b"corrupt index").unwrap();

        repository.load_index_from_path(&path).unwrap();

        assert!(!path.exists());
    }

    #[test]
    fn test_index_io_failure() {
        let location = RepositoryFixture::new("repo").write().unwrap();
        let mut repository = Repository::load("repo", &location).unwrap();
        let directory = tempdir().unwrap();
        assert!(matches!(
            repository.write_index_to_path(&directory.path().join("missing/index"), true),
            Err(RepositoryError::Data(_))
        ));
        assert!(matches!(
            repository.load_index_from_path(directory.path()),
            Err(RepositoryError::Data(_))
        ));
    }

    #[test]
    fn test_repository_equality() {
        let gentoo = Repository {
            name: "gentoo".into(),
            ..Default::default()
        };
        let guru = Repository {
            name: "guru".into(),
            ..Default::default()
        };
        assert_ne!(gentoo, guru);
        assert_eq!(
            gentoo,
            Repository {
                name: "gentoo".into(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn test_repository_display() {
        let repo = Repository {
            name: "gentoo".into(),
            ..Default::default()
        };
        assert_eq!(repo.to_string(), "gentoo");
    }

    #[test]
    fn test_optional_data_validation() {
        let valid_location = RepositoryFixture::new("valid").write().unwrap();
        let invalid_location = RepositoryFixture::new("invalid")
            .formats(["pms"])
            .eapi("0")
            .profile_entries_dir("package.mask", "app-misc/foo\n")
            .write()
            .unwrap();

        let valid = Repository::load("valid", &valid_location);
        let invalid = Repository::load("invalid", &invalid_location);

        assert!(valid.is_ok());
        assert!(matches!(invalid, Err(RepositoryError::Profile(_))));
    }

    #[test]
    fn test_portage_mask_directories() {
        for format in ["portage-1", "portage-2"] {
            let location = RepositoryFixture::new("repo")
                .formats([format])
                .eapi("0")
                .profile_entries_dir("package.mask", "app-misc/foo\n")
                .write()
                .unwrap();

            Repository::load("repo", &location).unwrap();
        }
    }

    #[test]
    fn test_eapi_mask_directories() {
        for eapi in ["7", "8"] {
            let location = RepositoryFixture::new("repo")
                .formats(["pms"])
                .eapi(eapi)
                .profile_entries_dir("package.mask", "app-misc/foo\n")
                .write()
                .unwrap();
            Repository::load("repo", &location).unwrap();
        }
    }
}
