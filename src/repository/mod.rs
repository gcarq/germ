mod config;
mod desc;
mod eclass;
pub mod manager;
mod sync;

use crate::conf::PortageConf;
use crate::deps::Atom;
use crate::eapi::Eapi;
use crate::ebuild::Ebuild;
use crate::ebuild::handler::{EbuildPhase, EbuildPhaseHandler};
use crate::linefile::LineBasedFile;
use crate::package::Package;
use crate::package::version::PackageVersion;
use crate::regex::PKG_VER_REV;
use crate::repository::config::RepositoryConfig;
use crate::repository::desc::ProfileDescription;
use crate::repository::eclass::Eclasses;
use crate::repository::manager::RepoManager;
use crate::repository::sync::{SyncHandler, build_sync_handler};
use crate::utils;
use crate::utils::FileFromPath;
use anyhow::{Context, Result, anyhow};
use lazy_static::lazy_static;
use log::info;
use regex::Regex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::{fmt, fs, iter};

lazy_static! {
    /// Regex to validate and parse `package`, `version`, `suffixes` and the `revision`
    /// from an ebuild name.
    static ref EBUILD_RE: Regex = Regex::new(&format!(r"^{PKG_VER_REV}.ebuild$")).unwrap();
}

/// Represents a package repository with its location, name, eapi version, categories, packages,
/// and other metadata. The repository will be synced using a [`SyncHandler`].
pub struct Repository {
    pub location: PathBuf,
    pub name: String,
    masters: Vec<String>,
    pub eapi: Eapi,
    categories: Vec<String>,
    packages: HashSet<Package>,
    pub package_mask: LineBasedFile,
    pub package_unmask: LineBasedFile,
    eclasses: Eclasses,
    pub arch_list: LineBasedFile,
    pub profiles_desc: Vec<ProfileDescription>,

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
            packages: HashSet::new(),
            categories: Vec::new(),
            package_mask: LineBasedFile::from_path(
                &profiles.join("package.mask"),
                eapi.profile_file_dirs,
                true,
            )?,
            package_unmask: LineBasedFile::from_path(
                &profiles.join("package.unmask"),
                eapi.profile_file_dirs,
                true,
            )?,
            eclasses: Eclasses::from_path(&location.join("eclass"))
                .with_context(|| "unable to collectd eclasses")?,
            arch_list: LineBasedFile::from_path(&profiles.join("arch.list"), false, true)?,
            profiles_desc: LineBasedFile::from_path(&profiles.join("profiles.desc"), false, true)?
                .into_iter()
                .map(|line| ProfileDescription::from_line(&line))
                .collect::<Result<_>>()?,
            masters: config.masters.clone(),
            name: config.name.clone(),
            location,
            sync_handler: build_sync_handler(&config.raw_properties)?,
            eapi,
        };
        Ok(repository)
    }

    /// Populates all categories and packages. Categories are inherited from the given `masters`.
    pub fn populate(&mut self, masters: &[&Repository]) -> Result<()> {
        self.collect_categories(masters)?;
        self.collect_packages()
    }

    /// Returns an `Iterator` over all packages in the repository.
    /// TODO: Order the returned packages by version
    pub fn packages(&self) -> impl Iterator<Item = &Package> {
        self.packages.iter()
    }

    /// Returns all packages in the repository that match the given `atom`.
    /// TODO: Order the returned packages by version
    /// TODO: Consider returning an iterator
    pub fn find_packages(&self, atom: &Atom) -> Vec<&Package> {
        self.packages().filter(|pkg| atom.matches(pkg)).collect()
    }

    /// Checks if the profile with the relative `profile_path` is valid for the given `arch`.
    /// The repository location prefix must be stripped from the passed `profile_path` string
    /// e.g.: default/linux/23.0
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

    /// Resolves the [`Ebuild`] for the given `package`.
    /// Returns Err if the ebuild file doesn't exist or is invalid.
    fn resolve_ebuild<'p>(&self, package: &'p Package) -> Result<Ebuild<'p>> {
        let path = self
            .location
            .join(&package.category)
            .join(&package.name)
            .join(format!("{}-{}.ebuild", package.name, package.version));
        Ebuild::new(path, package)
    }

    /// Generates metadata cache for all packages in the repository.
    /// TODO: save metadata
    fn generate_metadata(&self, conf: &PortageConf) -> Result<()> {
        info!("Generating metadata cache for repository {self} ...");
        for pkg in self.packages() {
            let ebuild = self.resolve_ebuild(pkg)?;
            let mut handler = EbuildPhaseHandler::new(&ebuild, conf, EbuildPhase::Depend)
                .with_context(|| anyhow!("unable to generate metadata for '{pkg}'"))?;
            handler.execute()?;
        }
        Ok(())
    }

    /// Collects all categories from the repository.
    /// Categories from the given `masters` are inherited and added to the collected categories.
    fn collect_categories(&mut self, masters: &[&Repository]) -> Result<()> {
        self.categories
            .extend(masters.iter().flat_map(|repo| &repo.categories).cloned());

        let path = self.location.join("profiles").join("categories");
        if !path.exists() {
            return Ok(());
        }

        self.categories.extend(
            fs::read_to_string(&path)
                .with_context(|| anyhow!("unable to read '{}'", path.display()))?
                .lines()
                .map(|line| line.to_owned()),
        );
        Ok(())
    }

    /// Collects all packages from the repository.
    /// [`Self::collect_categories`] must be called before calling this method since only
    /// known categories are considered when collecting packages.
    fn collect_packages(&mut self) -> Result<()> {
        for category in &self.categories {
            let cat_path = self.location.join(category);

            let pkg_paths = match utils::list_dirs(&cat_path) {
                Ok(paths) => paths,
                _ => continue,
            };

            for pkg_path in pkg_paths {
                let pkg_path = pkg_path?;
                let pkg_name = utils::path_to_filename(&pkg_path)?;

                for file_path in utils::list_files(&pkg_path)? {
                    let file_path = file_path?;
                    let caps = match EBUILD_RE.captures(utils::path_to_filename(&file_path)?) {
                        Some(caps) if caps["package"].starts_with(pkg_name) => caps,
                        _ => continue,
                    };
                    let version = PackageVersion::new(
                        &caps["version"],
                        Some(&caps["suffixes"]),
                        caps.name("revision").map(|m| m.as_str()),
                    )?;
                    let pkg = Package::new(
                        utils::path_to_filename(&cat_path)?,
                        pkg_name,
                        version,
                        &self.name,
                    )?;
                    self.packages.insert(pkg);
                }
            }
        }
        Ok(())
    }

    /// Resolves all masters recursively and returns an `Iterator` with `self`
    /// and all resolved Repositories.
    ///
    /// To passed [`RepoManager`] is needed to resolve repositories.
    /// NOTE: If a repository is listed as a master but doesn't exist, it will be silently ignored.
    fn resolve_masters<'a>(
        &'a self,
        repo_manager: &'a RepoManager,
    ) -> Box<dyn Iterator<Item = &'a Repository> + 'a> {
        let it = iter::once(self).chain(
            self.masters
                .iter()
                .filter_map(|name| repo_manager.get_repo(name))
                .flat_map(|repo| repo.resolve_masters(repo_manager)),
        );
        Box::new(it)
    }

    /// Reads the repository eapi version from the given repository `path`.
    /// Returns `Eapi::default()` if no eapi file exists.
    fn read_eapi(path: &Path) -> Result<Eapi> {
        let eapi_file = path.join("profiles").join("eapi");
        if !fs::exists(&eapi_file)? {
            return Ok(Eapi::default());
        }
        Eapi::new(
            fs::read_to_string(&eapi_file)?
                .lines()
                .next()
                .ok_or_else(|| anyhow!("Empty eapi file"))?,
        )
    }
}

impl fmt::Display for Repository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
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
