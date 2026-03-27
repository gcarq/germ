mod config;
mod desc;
pub mod eclass;
mod index;
mod misc;
pub mod set;
mod sync;
mod utils;

use crate::consts::DEFAULT_CACHE_PATH;
use crate::deps::atom::Atom;
use crate::eapi::Eapi;
use crate::files::{FileFromPath, PackageEntries};
use crate::package::Package;
use crate::package::cpv::CPV;
use crate::regex::PV_REV;
use crate::repository::config::RepositoryConfig;
use crate::repository::desc::ProfileDescriptions;
use crate::repository::eclass::Eclasses;
use crate::repository::index::{AvailablePackageIndex, ResolvedPackageIndex};
use crate::repository::misc::ArchList;
use crate::repository::sync::{SyncHandler, build_sync_handler};
use crate::repository::utils::resolve_cpv_from_category;
use crate::utils::Inherit;
use anyhow::{Context, Result, anyhow};
use log::{debug, info};
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use regex::Regex;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::{fmt, fs};

/// Regex to validate and parse `package`, `version`, `suffixes` and the `revision`
/// from an ebuild name.
static EBUILD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^{PV_REV}.ebuild$")).unwrap());

/// Represents a package repository with its location, name, eapi version, categories, packages,
/// and other metadata. The repository will be synced using a [`SyncHandler`].
#[cfg_attr(test, derive(Default, Debug))]
pub struct Repository {
    pub location: PathBuf,
    pub name: String,
    categories: Vec<String>,
    pub package_mask: PackageEntries,
    pub package_unmask: PackageEntries,
    pub eclasses: Eclasses,
    pub arch_list: ArchList,
    pub profiles_desc: ProfileDescriptions,
    avail_package_idx: AvailablePackageIndex,
    resolved_package_idx: ResolvedPackageIndex,

    sync_handler: Option<Box<dyn SyncHandler>>,
}

impl Repository {
    /// Builds a new [`Repository`] with the given `location` and INI `properties` from repos.conf.
    ///
    /// Packages must be collected separately by calling `collect_packages` since they require
    /// parsing the whole repository and can be expensive to build.
    /// This allows deferring package collection until it's actually needed.
    pub fn new(config: &RepositoryConfig) -> Result<Self> {
        let location = config.location.canonicalize()?;

        let eapi = Self::read_eapi(&location)?;
        let profiles = location.join("profiles");
        let repository = Self {
            categories: Vec::default(),
            package_mask: PackageEntries::from_path(
                &profiles.join("package.mask"),
                eapi.supports_profile_file_dirs(),
                true,
            )?,
            package_unmask: PackageEntries::from_path(
                &profiles.join("package.unmask"),
                eapi.supports_profile_file_dirs(),
                true,
            )?,
            eclasses: Eclasses::default(),
            arch_list: ArchList::from_path(&profiles.join("arch.list"), false, true)?,
            profiles_desc: ProfileDescriptions::from_path(
                &profiles.join("profiles.desc"),
                false,
                true,
            )?,
            name: config.name.clone(),
            avail_package_idx: AvailablePackageIndex::default(),
            resolved_package_idx: ResolvedPackageIndex::default(),
            sync_handler: build_sync_handler(&config.raw_properties)?,
            location,
        };
        Ok(repository)
    }

    /// Finds and returns all packages that match the given `atom`.
    /// TODO: optimize this function
    pub fn find_packages(&mut self, atom: &Atom) -> Result<Vec<&Package>> {
        let Some(cpvs) = self.avail_package_idx.find_packages(atom) else {
            return Ok(Vec::new());
        };

        // Resolve all packages first to avoid borrowing conflicts when collecting references later
        let packages = self.par_resolve_packages(cpvs.iter().copied())?;
        self.resolved_package_idx.extend(packages);

        cpvs.into_iter()
            .map(|cpv| -> Result<&Package> {
                self.resolved_package_idx
                    .get(cpv.fqn())
                    .ok_or_else(|| anyhow!("package {cpv} not found"))
            })
            .collect()
    }

    /// Builds the resolved package index for all packages.
    pub fn build_package_index(&mut self) -> Result<()> {
        let cpvs = self.avail_package_idx.values().flatten();
        let resolved = self.par_resolve_packages(cpvs)?;
        self.resolved_package_idx.extend(resolved);
        Ok(())
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

    /// Synchronizes the repository using its [`SyncHandler`].
    pub fn sync(&self) -> Result<()> {
        if let Some(sync_handler) = &self.sync_handler {
            info!("Syncing repository '{}'", self.name);
            sync_handler.sync()?;
        }
        Ok(())
    }

    /// Resolves the given `cpvs` in parallel using `rayon`.
    ///
    /// Only unresolved packages will be resolved.
    fn par_resolve_packages<'a>(
        &self,
        cpvs: impl Iterator<Item = &'a CPV>,
    ) -> Result<Vec<Package>> {
        let filtered = cpvs
            .filter(|cpv| !self.resolved_package_idx.contains_key(cpv.fqn()))
            .collect::<Vec<_>>();
        if filtered.is_empty() {
            return Ok(Vec::new());
        }

        filtered
            .into_par_iter()
            .map(|cpv| self.resolve_package(cpv))
            .collect::<Result<Vec<_>>>()
            .with_context(|| "unable to resolve packages")
    }

    /// Resolves the given `cpv` into a [`Package`] with its metadata.
    fn resolve_package(&self, cpv: &CPV) -> Result<Package> {
        let ebuild_path = self
            .location
            .join(cpv.category())
            .join(cpv.package())
            .join(format!("{}.ebuild", cpv.pf()));
        let metadata = cpv
            .generate_metadata(&ebuild_path, self)
            .with_context(|| anyhow!("unable to generate metadata for {cpv}"))?;
        Ok(Package::new(cpv.clone(), self.name.clone(), metadata))
    }

    /// Writes the resolved package index to disk.
    ///
    /// If `force` is `true`, the index will be written even if it hasn't been modified
    /// since the last write.
    ///
    /// Returns `Err` if the index cannot be serialized or the file cannot be created.
    fn write_index(&self, force: bool) -> Result<()> {
        let meta_dir = PathBuf::from(DEFAULT_CACHE_PATH).join("metadata");
        if !fs::exists(&meta_dir)? {
            fs::create_dir_all(&meta_dir)
                .with_context(|| anyhow!("unable to create directory: {}", meta_dir.display()))?;
        }
        debug!(
            "Writing package index for '{self}' into {} ...",
            meta_dir.display()
        );
        self.resolved_package_idx
            .write_to_path(&meta_dir.join(&self.name), force)
            .with_context(|| anyhow!("failed to write package index for '{self}'"))?;
        Ok(())
    }

    /// Loads the resolved package index from disk.
    ///
    /// Returns `Err` if the index cannot be deserialized or the file exists but cannot be opened.
    fn load_index(&mut self) -> Result<()> {
        let path = PathBuf::from(DEFAULT_CACHE_PATH)
            .join("metadata")
            .join(&self.name);
        debug!(
            "Loading package index for '{self}' from {} ...",
            path.display()
        );
        if let Some(index) = ResolvedPackageIndex::load_from_path(&path)? {
            self.resolved_package_idx = index;
            self.resolved_package_idx.retain(&self.avail_package_idx);
        }
        Ok(())
    }

    /// Populates all categories, packages and eclasses.
    ///
    /// NOTE: The caller must ensure [`Inherit::inherit_from`] has been called before.
    fn populate(&mut self) -> Result<()> {
        self.collect_eclasses()
            .with_context(|| "unable to collect eclasses")?;
        self.collect_categories();
        self.collect_cpvs()
            .with_context(|| "unable to collect packages")?;
        Ok(())
    }

    /// Collects all known eclasses in the repo.
    fn collect_eclasses(&mut self) -> Result<()> {
        let eclasses = Eclasses::from_path(&self.location.join("eclass"))?;
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
    fn collect_cpvs(&mut self) -> Result<()> {
        let packages = self
            .categories
            .par_iter()
            .flat_map_iter(|category| resolve_cpv_from_category(&self.location, category))
            .collect::<Result<Vec<_>>>()?;
        self.avail_package_idx.insert_all(packages);
        Ok(())
    }

    /// Reads the repository eapi version from the given repository `path`.
    ///
    /// Returns `Eapi::default()` if no eapi file exists.
    fn read_eapi(path: &Path) -> Result<Eapi> {
        let eapi_file = path.join("profiles").join("eapi");
        if !fs::exists(&eapi_file)? {
            return Ok(Eapi::default());
        }
        fs::read_to_string(&eapi_file)?
            .lines()
            .next()
            .ok_or_else(|| anyhow!("Empty eapi file"))?
            .parse()
    }
}

impl Inherit for Repository {
    /// Inherits relevant metadata from the given `master` repository.
    fn inherit_from(&mut self, master: &Repository) {
        debug!("Inheriting '{self}' from '{master}' ...");
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
}
