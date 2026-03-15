mod config;
mod desc;
pub mod eclass;
mod index;
pub mod set;
mod sync;

use crate::consts::DEFAULT_CACHE_PATH;
use crate::deps::atom::Atom;
use crate::eapi::Eapi;
use crate::linefile::LineBasedFile;
use crate::package::Package;
use crate::package::cpv::CPV;
use crate::package::version::PackageVersion;
use crate::regex::PV_REV;
use crate::repository::config::RepositoryConfig;
use crate::repository::desc::ProfileDescription;
use crate::repository::eclass::Eclasses;
use crate::repository::index::{AvailablePackageIndex, ResolvedPackageIndex};
use crate::repository::sync::{SyncHandler, build_sync_handler};
use crate::utils;
use crate::utils::{FileFromPath, Inherit};
use anyhow::{Context, Result, anyhow};
use log::{debug, info};
use regex::Regex;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::str::FromStr;
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
    eapi: Eapi,
    categories: Vec<String>,
    pub package_mask: LineBasedFile,
    pub package_unmask: LineBasedFile,
    pub eclasses: Eclasses,
    pub arch_list: LineBasedFile,
    pub profiles_desc: Box<[ProfileDescription]>,
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
            package_mask: LineBasedFile::from_path(
                &profiles.join("package.mask"),
                eapi.supports_profile_file_dirs(),
                true,
            )?,
            package_unmask: LineBasedFile::from_path(
                &profiles.join("package.unmask"),
                eapi.supports_profile_file_dirs(),
                true,
            )?,
            eclasses: Eclasses::default(),
            arch_list: LineBasedFile::from_path(&profiles.join("arch.list"), false, true)?,
            profiles_desc: LineBasedFile::from_path(&profiles.join("profiles.desc"), false, true)?
                .into_iter()
                .map(|line| ProfileDescription::from_line(&line))
                .collect::<Result<_>>()?,
            name: config.name.clone(),
            avail_package_idx: AvailablePackageIndex::default(),
            resolved_package_idx: ResolvedPackageIndex::default(),
            sync_handler: build_sync_handler(&config.raw_properties)?,
            location,

            eapi,
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
        for cpv in cpvs.iter() {
            if !self.resolved_package_idx.contains_key(cpv.fqn()) {
                self.resolved_package_idx.insert(self.resolve_package(cpv)?);
            }
        }

        cpvs.iter()
            .map(|cpv| -> Result<&Package> {
                self.resolved_package_idx
                    .get(cpv.fqn())
                    .ok_or_else(|| anyhow!("package {cpv} not found"))
            })
            .collect()
    }

    /// Builds the resolved package index for all packages.
    pub fn build_package_index(&mut self) -> Result<()> {
        for cpv in self.avail_package_idx.values().flatten() {
            if !self.resolved_package_idx.contains_key(cpv.fqn()) {
                self.resolved_package_idx.insert(self.resolve_package(cpv)?);
            }
        }
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
        Ok(Package {
            cpv: cpv.clone(),
            repo: self.name.clone(),
            metadata,
        })
    }

    /// Writes the resolved package index to disk.
    fn write_index(&self, force: bool) -> Result<()> {
        let path = PathBuf::from(DEFAULT_CACHE_PATH)
            .join("metadata")
            .join(&self.name);
        self.resolved_package_idx.write_to_path(&path, force)?;
        Ok(())
    }

    /// Loads the resolved package index from disk.
    fn load_index(&mut self) -> Result<()> {
        let path = PathBuf::from(DEFAULT_CACHE_PATH)
            .join("metadata")
            .join(&self.name);
        if let Some(index) = ResolvedPackageIndex::load_from_path(&path)? {
            self.resolved_package_idx = index;
        }
        self.resolved_package_idx.retain(&self.avail_package_idx);

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
        for category in &self.categories {
            let cat_path = self.location.join(category);
            let Ok(pkg_paths) = utils::list_dirs(&cat_path) else {
                continue;
            };

            for pkg_path in pkg_paths {
                let pkg_path = pkg_path?;
                let package = utils::path_to_filename(&pkg_path)?;

                for ebuild_path in utils::list_files(&pkg_path)? {
                    let ebuild_path = ebuild_path?;
                    let caps = match EBUILD_RE.captures(utils::path_to_filename(&ebuild_path)?) {
                        Some(caps) if caps["package"].starts_with(package) => caps,
                        _ => continue,
                    };
                    let version = PackageVersion::new(
                        &caps["version"],
                        Some(&caps["suffixes"]),
                        caps.name("revision").map(|m| m.as_str()),
                    )?;
                    let cpv = CPV::new(category, package, version)?;
                    self.avail_package_idx.insert(cpv);
                }
            }
        }
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
        Eapi::from_str(
            fs::read_to_string(&eapi_file)?
                .lines()
                .next()
                .ok_or_else(|| anyhow!("Empty eapi file"))?,
        )
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
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
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
    fn test_ebuild_regex_match() {
        let valid_ebuilds = [
            "vim-8.2.3456.ebuild",
            "vim-8.2.3456-r1.ebuild",
            "rust-1.65.0_alpha1-r2.ebuild",
            "curl-7.79.1_beta2_p20220101.ebuild",
        ];
        for ebuild in valid_ebuilds {
            assert!(
                EBUILD_RE.is_match(ebuild),
                "ebuild name '{ebuild}' should be valid",
            );
        }
    }

    #[test]
    fn test_ebuild_regex_no_match() {
        let invalid_ebuilds = [
            "",
            "vim8.2.3456.ebuild",
            "app-editors/vim-.ebuild",
            "dev-lang/rust-1.65.0_alphaX-r2.ebuild",
            "net-misc/curl-7.79.1--r1.ebuild",
            "net-misc/curl-7.79.1_beta2_p20220101-rX.ebuild",
        ];
        for ebuild in invalid_ebuilds {
            assert!(
                !EBUILD_RE.is_match(ebuild),
                "ebuild name '{ebuild}' should be invalid",
            );
        }
    }
}
