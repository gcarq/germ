mod eclass;
mod error;
mod layout;
mod package;
mod profiles;

pub use eclass::{Eclass, Eclasses};
use either::Either;
pub use error::RepositoryError;
use futures_util::{StreamExt, TryStreamExt, stream};
pub use layout::{Layout, LayoutError};
pub use package::{PackageResolutionError, PackageResult};
pub use profiles::{Arch, Arches, ProfileError};

use self::package::CPVIndex;
use self::profiles::ProfileDescriptions;
use crate::SysConf;
use crate::deps::atom::Atom;
use crate::eapi::Eapi;
use crate::ebuild::Ebuild;
use crate::files::{PackageEntries, entry::Precedence};
use crate::package::PackageView;
use crate::package::names::CatName;
use crate::package::{Package, cpv::CPV};
use crate::repository::RepoName;
use crate::repository::tree::package::cache::MetadataCache;
use crate::repository::tree::package::discovery::{
    resolve_from_category_path, resolve_from_pkg_path, resolve_from_repo_path,
};
use crate::types::FxHashSet;
use crate::utils::{Inherit, is_blank_or_comment};
use anyhow::{Context, anyhow};
use log::{debug, warn};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fmt, fs};

pub use package::cache::CacheError;

/// Represents an available ebuild repository.
/// See https://projects.gentoo.org/pms/8/pms.html#x1-290004.1
#[derive(Debug)]
pub struct Repository {
    location: PathBuf,
    name: RepoName,
    layout: Layout,
    eclasses: Eclasses,
    supported_arches: Arches,
    known_categories: FxHashSet<CatName>,
    profiles_desc: ProfileDescriptions,
    cpv_index: CPVIndex,
    metadata_cache: MetadataCache,
    sysconf: Arc<SysConf>,

    pub(super) package_mask: PackageEntries,
    pub(super) package_unmask: PackageEntries,
}

impl Repository {
    /// Loads repository data from disk with the given [`SysConf`].
    pub fn load(
        name: &RepoName,
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

        let name = Self::resolve_repo_name(name, &layout, &profiles)?;

        Ok(Self {
            location: location.to_owned(),
            metadata_cache: MetadataCache::new(&location.join("cache")),
            known_categories: FxHashSet::default(),
            eclasses: Eclasses::empty(location),
            supported_arches: Arches::from_path(&profiles.join("arch.list"))?,
            profiles_desc: ProfileDescriptions::from_path(&profiles.join("profiles.desc"))?,
            cpv_index: CPVIndex::default(),
            package_mask,
            package_unmask,
            layout,
            name,
            sysconf,
        })
    }

    /// Returns all existing CPVs in the repository.
    pub fn cpvs(&mut self) -> impl Iterator<Item = &CPV> {
        self.ensure_discovered_cpvs(&Atom::default());
        self.cpv_index.iter()
    }

    /// Eagerly resolves and returns all known packages.
    pub async fn packages(&mut self) -> Result<Vec<PackageResult>, RepositoryError> {
        self.find_packages(&Atom::default()).await
    }

    /// Eagerly resolves all packages that match the given [`Atom`].
    pub async fn find_packages(
        &mut self,
        atom: &Atom,
    ) -> Result<Vec<PackageResult>, RepositoryError> {
        self.ensure_discovered_cpvs(atom);
        let cpvs = self.cpv_index.find_packages(atom);
        self.resolve_packages(cpvs).await
    }

    /// Returns the [`RepoName`].
    pub const fn name(&self) -> &RepoName {
        &self.name
    }

    /// Returns the location on disk.
    pub fn location(&self) -> &Path {
        &self.location
    }

    /// Returns all defined eclasses.
    pub const fn eclasses(&self) -> &Eclasses {
        &self.eclasses
    }

    /// Returns the current [`Layout`].
    pub const fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Checks if the profile with the relative `rel_path` is valid for the given `arch`.
    ///
    /// The repository location prefix must be stripped from the passed `rel_path`,
    /// e.g.: `default/linux/23.0`
    pub fn is_known_profile(&self, arch: &Arch, rel_path: &Path) -> bool {
        self.profiles_desc
            .iter()
            .any(|desc| &desc.arch == arch && desc.profile_path.as_str() == rel_path.as_os_str())
    }

    /// Checks if the repository supports the given `arch`.
    pub fn supports_arch(&self, arch: &Arch) -> bool {
        self.supported_arches.contains(arch)
    }

    /// Resolves all configured categories and eclasses and also clears the CPV index.
    ///
    /// NOTE: The caller must ensure [`Inherit::inherit_from`] has been called before.
    pub fn finalize(&mut self) -> Result<(), RepositoryError> {
        self.collect_eclasses().map_err(RepositoryError::Data)?;
        self.collect_categories();
        self.cpv_index.clear();
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
        self.ensure_discovered_cpvs(&Atom::default());
        self.metadata_cache.retain(self.cpv_index.iter())?;
        self.metadata_cache.compact()?;
        Ok(())
    }

    /// Resolves the [`Package`] for the given [`CPV`].
    async fn resolve_package<'r>(&'r self, cpv: &'r CPV) -> PackageResult {
        let ebuild = match Ebuild::new(cpv, self) {
            Ok(ebuild) => ebuild,
            Err(error) => {
                return Err(PackageResolutionError::new(cpv.fqn(), error.into()));
            }
        };

        match ebuild.generate_metadata().await {
            Ok(metadata) => Ok(Package::new(cpv.to_owned(), self.name.clone(), metadata)),
            Err(source) => Err(PackageResolutionError::new(cpv.fqn(), source)),
        }
    }

    /// Builds the [`Package`] index for the given `cpvs`.
    async fn resolve_packages<'r>(
        &'r self,
        cpvs: impl Iterator<Item = &'r CPV>,
    ) -> Result<Vec<PackageResult>, RepositoryError> {
        let mut cached = Vec::with_capacity(cpvs.size_hint().0);
        let mut missing = Vec::new();

        for cpv in cpvs {
            match self.metadata_cache.get(cpv)? {
                Some(metadata) => {
                    cached.push(Ok(Package::new(
                        cpv.to_owned(),
                        self.name.clone(),
                        metadata,
                    )));
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
                .map(|pkg| (pkg.cpv(), pkg.metadata())),
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
            let iter = content
                .lines()
                .map(str::trim)
                .filter(|line| !is_blank_or_comment(line))
                .filter_map(|category| match category.parse() {
                    Ok(cat) => Some(cat),
                    Err(err) => {
                        warn!("invalid category in {}: {err}. Skipping...", path.display());
                        None
                    }
                });
            self.known_categories.extend(iter);
        }
    }

    /// Ensures all [`CPV`] that match the given [`Atom`]
    /// are discovered.
    ///
    /// TODO: Handle `*/package` more efficiently.
    fn ensure_discovered_cpvs(&mut self, atom: &Atom) {
        if self.cpv_index.is_discovered(atom) {
            return;
        }

        let cpvs = match (atom.category(), atom.package()) {
            (Some(cat), Some(pkg)) => Either::Left(resolve_from_pkg_path(
                cat,
                pkg.clone(),
                &self.location.join(cat.as_str()).join(pkg.as_str()),
            )),
            (Some(cat), None) => Either::Right(Either::Left(resolve_from_category_path(
                cat,
                &self.location.join(cat.as_str()),
            ))),
            (None, _) => Either::Right(Either::Right(resolve_from_repo_path(
                &self.location,
                &self.known_categories,
            ))),
        };

        self.cpv_index.insert(cpvs);
        self.cpv_index.sort();
        self.cpv_index.mark_discovered(atom);
    }

    /// Resolves the repo name and validates it against `profiles/repo_name` and `layout.conf`.
    ///
    /// The given `name` should be the name of the repository as defined in `repos.conf`.
    fn resolve_repo_name(
        name: &RepoName,
        layout: &Layout,
        profiles: &Path,
    ) -> Result<RepoName, ProfileError> {
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
                .map(str::trim)
                .map(str::parse)??
        };
        if declared_name != *name {
            warn!(
                "Repository name mismatch: repo_name='{declared_name}' vs repos.conf='{name}'! Using {name}..."
            );
        }
        Ok(name.clone())
    }
}

impl Inherit for Repository {
    /// Inherits relevant metadata from the given `master` repository.
    fn inherit_from(&mut self, master: &Repository) -> anyhow::Result<()> {
        debug!("Inheriting '{}' from '{}' ...", self.name, master.name);
        self.known_categories
            .extend(master.known_categories.iter().cloned());
        self.eclasses.extend(&master.eclasses);
        self.supported_arches.extend(&master.supported_arches);
        Ok(())
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
        f.write_str(self.name.as_str())
    }
}

#[cfg(test)]
impl Default for Repository {
    fn default() -> Self {
        let temp_dir = tempfile::Builder::new()
            .tempdir()
            .expect("failed to create temp dir");
        let metadata_cache = MetadataCache::new(&temp_dir.path().join("metadata"));
        Self {
            name: "repo".parse().unwrap(),
            location: temp_dir.path().to_owned(),
            layout: Layout::default(),
            known_categories: FxHashSet::default(),
            package_mask: PackageEntries::default(),
            package_unmask: PackageEntries::default(),
            eclasses: Eclasses::default(),
            supported_arches: Arches::default(),
            profiles_desc: ProfileDescriptions::default(),
            cpv_index: CPVIndex::default(),
            sysconf: SysConf::default().into(),
            metadata_cache,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::iter;

    use super::*;

    use super::super::test_support::RepoBuilder;
    use crate::package::metadata::PackageMetadata;
    use crate::test_support::cpv;

    #[tokio::test]
    async fn test_repository_resolves_cached_metadata() {
        let repository = RepoBuilder::new("repo").finalize().unwrap();
        let cpv = cpv("app-misc", "foo", "1");
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
        assert_eq!(package.metadata(), &metadata);
    }

    #[test]
    fn test_repository_equality() {
        let gentoo = Repository {
            name: "gentoo".parse().unwrap(),
            ..Default::default()
        };
        let guru = Repository {
            name: "guru".parse().unwrap(),
            ..Default::default()
        };
        assert_ne!(gentoo, guru);
        assert_eq!(
            gentoo,
            Repository {
                name: "gentoo".parse().unwrap(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn test_repository_display() {
        let repo = Repository {
            name: "gentoo".parse().unwrap(),
            ..Default::default()
        };
        assert_eq!(repo.to_string(), "gentoo");
    }

    #[test]
    fn test_repository_collect_categories() {
        let temp = tempfile::tempdir().unwrap();
        let location = temp.path().join("repo");
        RepoBuilder::new("repo")
            .categories(["app-misc"])
            .write_to(&location)
            .unwrap();
        fs::write(
            location.join("profiles/categories"),
            "\n# comment\napp-misc\ninvalid category\n",
        )
        .unwrap();

        let mut repository = Repository::load(
            &RepoName::default(),
            &location,
            Arc::new(SysConf::default()),
        )
        .unwrap();
        repository.finalize().unwrap();

        assert!(
            repository
                .known_categories
                .contains(&"app-misc".parse().unwrap())
        );
        assert_eq!(repository.known_categories.len(), 1);
    }

    #[test]
    fn test_profile_directory_support() {
        for (format, eapi, supported) in [
            ("pms", "0", false),
            ("portage-1", "0", true),
            ("portage-2", "0", true),
            ("pms", "7", true),
        ] {
            let result = RepoBuilder::new("repo")
                .formats([format])
                .eapi(eapi)
                .profile_entries_dir("package.mask", "app-misc/foo\n")
                .finalize();

            assert_eq!(result.is_ok(), supported);
        }
    }
}
