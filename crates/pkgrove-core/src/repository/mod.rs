mod config;
mod desc;
pub mod eclass;
mod index;
mod layout;
mod misc;
pub mod set;
mod sync;
#[cfg(test)]
pub(crate) mod test_support;
mod utils;

use crate::consts::DEFAULT_CACHE_PATH;
use crate::deps::atom::Atom;
use crate::eapi::Eapi;
use crate::files::PackageEntries;
use crate::files::entry::Precedence;
use crate::package::Package;
use crate::package::cpv::CPV;
use crate::regex::{PV_REV, REPO_RE};
use crate::repository::config::RepositoryConfig;
use crate::repository::desc::ProfileDescriptions;
use crate::repository::eclass::Eclasses;
use crate::repository::index::{AvailablePackageIndex, ResolvedPackageIndex};
use crate::repository::layout::Layout;
use crate::repository::misc::ArchList;
use crate::repository::sync::{SyncHandler, build_sync_handler};
use crate::repository::utils::resolve_cpv_from_category;
use crate::utils::Inherit;
use anyhow::{Context, Result, anyhow};
use log::{debug, info, warn};
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

/// Represents a package repository with its location, name and optional sync handler.
///
/// When a repository has been added but the location is not existant it's considered `NotLoaded`.
#[cfg_attr(test, derive(Default, Debug))]
pub struct Repository {
    pub location: PathBuf,
    pub name: String,
    sync_handler: Option<Box<dyn SyncHandler>>,
    state: RepositoryState,
}

#[derive(Default)]
#[cfg_attr(test, derive(Debug))]
#[allow(clippy::large_enum_variant)]
enum RepositoryState {
    #[default]
    NotLoaded,
    Loaded(RepositoryData),
}

#[cfg_attr(test, derive(Debug))]
struct RepositoryData {
    layout: Layout,
    categories: Vec<String>,
    package_mask: PackageEntries,
    package_unmask: PackageEntries,
    eclasses: Eclasses,
    arch_list: ArchList,
    profiles_desc: ProfileDescriptions,
    avail_package_idx: AvailablePackageIndex,
    resolved_package_idx: ResolvedPackageIndex,
}

impl Repository {
    /// Builds a new unloaded [`Repository`] from repos.conf data.
    pub fn new(config: &RepositoryConfig) -> Result<Self> {
        let sync_handler = build_sync_handler(config.raw_properties())?;
        Ok(Self {
            location: config.location.clone(),
            name: config.name.clone(),
            sync_handler,
            state: RepositoryState::NotLoaded,
        })
    }

    /// Returns whether repository metadata has been loaded from disk.
    pub const fn is_loaded(&self) -> bool {
        matches!(self.state, RepositoryState::Loaded(_))
    }

    /// Returns all known CPVs in the repository.
    pub fn cpvs(&self) -> Result<impl Iterator<Item = &CPV>> {
        Ok(self.data()?.avail_package_idx.values().flatten())
    }

    /// Returns repository package masks.
    pub fn package_mask(&self) -> Result<&PackageEntries> {
        Ok(&self.data()?.package_mask)
    }

    /// Returns repository package unmasks.
    pub fn package_unmask(&self) -> Result<&PackageEntries> {
        Ok(&self.data()?.package_unmask)
    }

    /// Returns repository eclasses.
    pub fn eclasses(&self) -> Result<&Eclasses> {
        Ok(&self.data()?.eclasses)
    }

    /// Returns the repository architecture list.
    pub fn arch_list(&self) -> Result<&ArchList> {
        Ok(&self.data()?.arch_list)
    }

    pub(super) fn layout(&self) -> Result<&Layout> {
        Ok(&self.data()?.layout)
    }

    /// Finds and returns all packages that match the given `atom`.
    /// TODO: optimize this function
    pub fn find_packages(&mut self, atom: &Atom) -> Result<Vec<&Package>> {
        let cpvs = match self.data()?.avail_package_idx.find_packages(atom) {
            Some(cpvs) => cpvs.into_iter().cloned().collect::<Vec<_>>(),
            None => return Ok(Vec::new()),
        };

        // Resolve all packages first to avoid borrowing conflicts when collecting references later
        let packages = self.par_resolve_packages(cpvs.iter())?;
        self.data_mut()?.resolved_package_idx.extend(packages);

        let data = self.data()?;
        cpvs.iter()
            .map(|cpv| -> Result<&Package> {
                data.resolved_package_idx
                    .get(cpv.fqn())
                    .ok_or_else(|| anyhow!("package {cpv} not found"))
            })
            .collect()
    }

    /// Builds the resolved package index for all packages.
    pub fn build_package_index(&mut self) -> Result<()> {
        let cpvs = self
            .data()?
            .avail_package_idx
            .values()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        let resolved = self.par_resolve_packages(cpvs.iter())?;
        self.data_mut()?.resolved_package_idx.extend(resolved);
        Ok(())
    }

    /// Checks if the profile with the relative `profile_path` is valid for the given `arch`.
    ///
    /// The repository location prefix must be stripped from the passed `profile_path` string
    /// e.g.: `default/linux/23.0`
    pub fn is_known_profile(&self, arch: &str, profile_path: &str) -> Result<bool> {
        Ok(self
            .data()?
            .profiles_desc
            .iter()
            .any(|desc| desc.keyword == arch && desc.profile_path == profile_path))
    }

    /// Synchronizes the repository using its [`SyncHandler`].
    fn sync(&self) -> Result<()> {
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
        let data = self.data()?;
        let filtered = cpvs
            .filter(|cpv| !data.resolved_package_idx.contains_key(cpv.fqn()))
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
        let data = self.data()?;
        let meta_dir = PathBuf::from(DEFAULT_CACHE_PATH).join("metadata");
        if !fs::exists(&meta_dir)? {
            fs::create_dir_all(&meta_dir)
                .with_context(|| anyhow!("unable to create directory: {}", meta_dir.display()))?;
        }
        debug!(
            "Writing package index for '{self}' into {} ...",
            meta_dir.display()
        );
        data.resolved_package_idx
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
            let data = self.data_mut()?;
            data.resolved_package_idx = index;
            data.resolved_package_idx.retain(&data.avail_package_idx);
        }
        Ok(())
    }

    fn data(&self) -> Result<&RepositoryData> {
        match &self.state {
            RepositoryState::Loaded(data) => Ok(data),
            RepositoryState::NotLoaded => Err(anyhow!(
                "repository '{}' at {} is not loaded; run sync first",
                self.name,
                self.location.display()
            )),
        }
    }

    fn data_mut(&mut self) -> Result<&mut RepositoryData> {
        match &mut self.state {
            RepositoryState::Loaded(data) => Ok(data),
            RepositoryState::NotLoaded => Err(anyhow!(
                "repository '{}' at {} is not loaded; run sync first",
                self.name,
                self.location.display()
            )),
        }
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
        self.data_mut()?.eclasses.extend(&eclasses);
        Ok(())
    }

    /// Collects all known categories in the repository.
    fn collect_categories(&mut self) {
        let path = self.location.join("profiles").join("categories");
        if let Ok(content) = fs::read_to_string(&path)
            && let Ok(data) = self.data_mut()
        {
            data.categories
                .extend(content.lines().map(ToOwned::to_owned));
        }
    }

    /// Collects all known packages in the repository as [`CPV`].
    ///
    /// NOTE: The caller must ensure to [`Self::collect_categories`] has been called before,
    /// since only known categories are considered when collecting packages.
    fn collect_cpvs(&mut self) -> Result<()> {
        let packages = self
            .data()?
            .categories
            .par_iter()
            .flat_map_iter(|category| resolve_cpv_from_category(&self.location, category))
            .collect::<Result<Vec<_>>>()?;
        self.data_mut()?.avail_package_idx.insert_all(packages);
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

    fn read_repo_name(location: &Path) -> Result<String> {
        fs::read_to_string(location.join("profiles").join("repo_name"))?
            .lines()
            .next()
            .ok_or_else(|| anyhow!("Empty repo_name file"))
            .map(ToOwned::to_owned)
    }

    fn validate_repo_name(name: &str) -> Result<()> {
        if !REPO_RE.is_match(name) {
            return Err(anyhow!(
                "Invalid repository name: {name}. It must match the regex: {}",
                REPO_RE.as_str()
            ));
        }
        Ok(())
    }

    fn load_data_from_disk(&mut self) -> Result<()> {
        let layout_path = &self.location.join("metadata").join("layout.conf");

        let layout = Layout::from_path(layout_path).with_context(|| {
            anyhow!("unable to load layout.conf from {}", layout_path.display())
        })?;

        // repo name from layout
        let name = match layout.name.as_ref() {
            Some(name) => name.clone(),
            None => Self::read_repo_name(&self.location)?,
        };
        Self::validate_repo_name(&name)?;
        if name != self.name {
            warn!(
                "Repository name mismatch: repo_name='{name}' vs repos.conf='{}'! Using {}...",
                self.name, self.name,
            );
        }

        let profiles = self.location.join("profiles");
        let dir_support = Self::read_eapi(&self.location)?.supports_profile_file_dirs();
        let data = RepositoryData {
            layout,
            categories: Vec::default(),
            package_mask: PackageEntries::from_path(
                &profiles.join("package.mask"),
                Precedence::Repository,
                dir_support,
            )?,
            package_unmask: PackageEntries::from_path(
                &profiles.join("package.unmask"),
                Precedence::Repository,
                dir_support,
            )?,
            eclasses: Eclasses::default(),
            arch_list: ArchList::from_path(&profiles.join("arch.list"))?,
            profiles_desc: ProfileDescriptions::from_path(&profiles.join("profiles.desc"))?,
            avail_package_idx: AvailablePackageIndex::default(),
            resolved_package_idx: ResolvedPackageIndex::default(),
        };
        self.state = RepositoryState::Loaded(data);
        Ok(())
    }
}

impl Inherit for Repository {
    /// Inherits relevant metadata from the given `master` repository.
    fn inherit_from(&mut self, master: &Repository) {
        match (&mut self.state, &master.state) {
            (RepositoryState::Loaded(data), RepositoryState::Loaded(master_data)) => {
                debug!("Inheriting '{}' from '{}' ...", self.name, &master.name);
                data.categories
                    .extend(master_data.categories.iter().cloned());
                data.eclasses.extend(&master_data.eclasses);
            }
            _ => debug!(
                "Skipping inheritance from '{}' to '{}' because one repository is not loaded",
                &master.name, self.name
            ),
        }
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
