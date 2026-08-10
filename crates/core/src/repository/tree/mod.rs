mod eclass;
mod error;
mod layout;
mod package;
mod profiles;

pub use eclass::{Eclass, Eclasses};
pub use error::RepositoryError;
use futures_util::{StreamExt, TryStreamExt, stream};
pub use layout::{Layout, LayoutError};
pub use package::{PackageResolutionError, PackageResult};
pub use profiles::{ArchList, ProfileError};

use self::package::{CPVIndex, resolve_cpv_from_category};
use self::profiles::ProfileDescriptions;
use crate::SysConf;
use crate::deps::atom::Atom;
use crate::eapi::Eapi;
use crate::ebuild::Ebuild;
use crate::files::{PackageEntries, entry::Precedence};
use crate::package::{Package, cpv::CPV};
use crate::regex::REPO_RE;
use crate::repository::tree::package::cache::MetadataCache;
use crate::types::FxHashSet;
use crate::utils::Inherit;
use anyhow::{Context, anyhow};
use log::{debug, warn};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fmt, fs};

pub use package::cache::CacheError;

/// Represents an available ebuild repository.
/// See https://projects.gentoo.org/pms/8/pms.html#x1-290004.1
#[derive(Debug)]
pub struct Repository {
    pub location: PathBuf,
    pub name: Arc<str>,
    pub layout: Layout,
    pub eclasses: Eclasses,
    pub package_mask: PackageEntries,
    pub package_unmask: PackageEntries,
    pub arch_list: ArchList,
    pub categories: FxHashSet<String>,
    profiles_desc: ProfileDescriptions,
    cpv_index: CPVIndex,
    metadata_cache: MetadataCache,
    sysconf: Arc<SysConf>,
}

impl Repository {
    /// Loads repository data from disk with the given [`SysConf`].
    pub fn load(
        name: &str,
        location: &Path,
        sysconf: Arc<SysConf>,
    ) -> Result<Self, RepositoryError> {
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

        let name = Self::resolve_repo_name(name, &layout, &profiles)?.into();

        Ok(Self {
            location: location.to_owned(),
            metadata_cache: MetadataCache::new(&location.join("cache"))?,
            categories: FxHashSet::default(),
            eclasses: Eclasses::empty(location),
            arch_list: ArchList::from_path(&profiles.join("arch.list"))?,
            profiles_desc: ProfileDescriptions::from_path(&profiles.join("profiles.desc"))?,
            cpv_index: CPVIndex::default(),
            package_mask,
            package_unmask,
            layout,
            name,
            sysconf,
        })
    }

    /// Returns all known CPVs in the repository.
    pub fn cpvs(&self) -> impl Iterator<Item = &CPV> {
        self.cpv_index.iter()
    }

    /// Eagerly resolves and returns all known packages.
    pub async fn packages<'r>(&'r self) -> Result<Vec<PackageResult<'r>>, RepositoryError> {
        self.resolve_packages(self.cpv_index.iter()).await
    }

    /// Eagerly resolves all packages that match the given [`Atom`].
    pub async fn find_packages<'r>(
        &'r self,
        atom: &Atom,
    ) -> Result<Vec<PackageResult<'r>>, RepositoryError> {
        self.resolve_packages(self.cpv_index.find_packages(atom))
            .await
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

    /// Populates all categories, packages and eclasses.
    ///
    /// NOTE: The caller must ensure [`Inherit::inherit_from`] has been called before.
    pub fn populate(&mut self) -> Result<(), RepositoryError> {
        self.collect_eclasses().map_err(RepositoryError::Data)?;
        self.collect_categories();
        self.collect_cpvs().map_err(RepositoryError::Data)?;
        Ok(())
    }

    /// Resolves all package metadata and builds [`MetadataCache`].
    pub async fn build_cache(&mut self) -> Result<Vec<PackageResolutionError>, RepositoryError> {
        Ok(self
            .packages()
            .await?
            .into_iter()
            .filter_map(Result::err)
            .collect())
    }

    /// Deletes and recreates the [`MetadataCache`].
    pub fn recreate_cache(&mut self) -> Result<(), CacheError> {
        self.metadata_cache.recreate()
    }

    /// Compacts the [`MetadataCache`] by removing all entries that are no longer valid,
    /// and reclaiming disk space if possible.
    pub fn compact_cache(&mut self) -> anyhow::Result<()> {
        self.metadata_cache.retain(self.cpvs())?;
        self.metadata_cache.compact()?;
        Ok(())
    }

    /// Resolves the [`Package`] for the given [`CPV`].
    async fn resolve_package<'r>(&'r self, cpv: &'r CPV) -> PackageResult<'r> {
        let ebuild = match Ebuild::new(cpv, self) {
            Ok(ebuild) => ebuild,
            Err(error) => {
                return Err(PackageResolutionError::new(cpv.fqn(), error.into()));
            }
        };

        match ebuild.generate_metadata().await {
            Ok(metadata) => Ok(Package::new(cpv, self.name.clone(), metadata)),
            Err(source) => Err(PackageResolutionError::new(cpv.fqn(), source)),
        }
    }

    /// Builds the [`Package`] index for the given `cpvs`.
    async fn resolve_packages<'r>(
        &'r self,
        cpvs: impl Iterator<Item = &'r CPV>,
    ) -> Result<Vec<PackageResult<'r>>, RepositoryError> {
        let mut cached = Vec::with_capacity(cpvs.size_hint().0);
        let mut missing = Vec::new();

        for cpv in cpvs {
            match self.metadata_cache.get(cpv)? {
                Some(metadata) => {
                    cached.push(Ok(Package::new(cpv, self.name.clone(), metadata)));
                }
                None => missing.push(cpv),
            }
        }

        let resolved = stream::iter(missing)
            .map(|cpv| async move {
                match self.resolve_package(cpv).await {
                    Ok(pkg) => Ok(Ok(pkg)),
                    Err(error) => error.promote().map(Err),
                }
            })
            .buffer_unordered(self.sysconf.ebuild_jobs())
            .try_collect::<Vec<_>>()
            .await?;

        self.metadata_cache.insert_batch(
            resolved
                .iter()
                .filter_map(|result| result.as_ref().ok())
                .map(|pkg| (pkg.cpv, &pkg.metadata)),
        )?;

        cached.extend(resolved);
        Ok(cached)
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
        self.cpv_index.insert_all(packages);
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
impl Default for Repository {
    fn default() -> Self {
        let temp_dir = tempfile::Builder::new()
            .tempdir()
            .expect("failed to create temp dir");
        let metadata_cache = MetadataCache::new(&temp_dir.path().join("metadata")).unwrap();
        Self {
            location: temp_dir.path().to_owned(),
            name: "".into(),
            layout: Layout::default(),
            categories: FxHashSet::default(),
            package_mask: PackageEntries::default(),
            package_unmask: PackageEntries::default(),
            eclasses: Eclasses::default(),
            arch_list: ArchList::default(),
            profiles_desc: ProfileDescriptions::default(),
            cpv_index: CPVIndex::default(),
            metadata_cache,
            sysconf: SysConf::default().into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::iter;

    use super::*;

    use super::super::test_support::RepoBuilder;
    use crate::{
        ebuild::{EbuildError, handler::error::MetadataGenerationError},
        package::{metadata::PackageMetadata, version::PackageVersion},
    };

    #[tokio::test]
    async fn test_invalid_package_versions() {
        let mut repository = RepoBuilder::new("repo")
            .categories(["app-misc"])
            .ebuild("app-misc", "foo", "1", "")
            .ebuild("app-misc", "foo", "2", "")
            .finalize()
            .unwrap();
        repository.populate().unwrap();

        let results = repository
            .find_packages(&Atom::new("app-misc/foo").unwrap())
            .await
            .unwrap();

        let errors = results
            .into_iter()
            .map(Result::unwrap_err)
            .collect::<Vec<_>>();

        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|error| matches!(
            &error.source,
            MetadataGenerationError::Ebuild(EbuildError::MissingEapi)
        )));
    }

    #[tokio::test]
    async fn test_repository_resolves_cached_metadata() {
        let repository = RepoBuilder::new("repo").finalize().unwrap();
        let cpv = CPV::new("app-misc", "foo", PackageVersion::try_from("1").unwrap()).unwrap();
        let metadata = PackageMetadata {
            description: "cached metadata".into(),
            ..Default::default()
        };
        repository
            .metadata_cache
            .insert_batch([(&cpv, &metadata)])
            .unwrap();
        let package = repository
            .resolve_packages(iter::once(&cpv))
            .await
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(package.metadata, metadata);
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
        let temp = tempfile::tempdir().unwrap();
        let valid_location = temp.path().join("valid");
        RepoBuilder::new("valid").write_to(&valid_location).unwrap();
        let invalid_location = temp.path().join("invalid");
        RepoBuilder::new("invalid")
            .formats(["pms"])
            .eapi("0")
            .profile_entries_dir("package.mask", "app-misc/foo\n")
            .write_to(&invalid_location)
            .unwrap();

        let sysconf = Arc::new(SysConf::default());
        let valid = Repository::load("valid", &valid_location, sysconf.clone());
        let invalid = Repository::load("invalid", &invalid_location, sysconf.clone());

        assert!(valid.is_ok());
        assert!(matches!(invalid, Err(RepositoryError::Profile(_))));
    }

    #[test]
    fn test_portage_mask_directories() {
        for format in ["portage-1", "portage-2"] {
            RepoBuilder::new("repo")
                .formats([format])
                .eapi("0")
                .profile_entries_dir("package.mask", "app-misc/foo\n")
                .finalize()
                .unwrap();
        }
    }

    #[test]
    fn test_eapi_mask_directories() {
        for eapi in ["7", "8"] {
            RepoBuilder::new("repo")
                .formats(["pms"])
                .eapi(eapi)
                .profile_entries_dir("package.mask", "app-misc/foo\n")
                .finalize()
                .unwrap();
        }
    }
}
