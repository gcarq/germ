mod desc;
mod eclass;
mod sync;

use crate::deps::Atom;
use crate::eapi::Eapi;
use crate::ebuild::Ebuild;
use crate::linefile::LineBasedFile;
use crate::package::Package;
use crate::package::version::PackageVersion;
use crate::regex::{PKG_VER_REV, REPOSITORY};
use crate::repository::desc::ProfileDescription;
use crate::repository::eclass::Eclasses;
use crate::repository::sync::{SyncHandler, build_sync_handler};
use crate::utils;
use crate::utils::FileFromPath;
use anyhow::{Context, Result, anyhow};
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

lazy_static! {
    /// Regex to validate repository names.
    static ref REPO_RE: Regex = Regex::new(&format!(r"^{REPOSITORY}$")).unwrap();

    /// Regex to validate and parse `package`, `version`, `suffixes` and the `revision`
    /// from an ebuild name.
    static ref EBUILD_RE: Regex = Regex::new(&format!(r"^{PKG_VER_REV}.ebuild$")).unwrap();
}

/// Represents a package repository with its location, name, eapi version, categories, packages,
/// and other metadata. The repository will be synced using a [`SyncHandler`].
pub struct Repository {
    pub location: PathBuf,
    pub name: String,
    pub priority: isize,
    pub eapi: Eapi,
    pub categories: Vec<String>,
    pub packages: HashSet<Package>,
    pub package_mask: LineBasedFile,
    pub package_unmask: LineBasedFile,
    pub eclasses: Eclasses,
    pub arch_list: LineBasedFile,
    pub profiles_desc: Vec<ProfileDescription>,

    sync_handler: Option<Box<dyn SyncHandler>>,
}

impl Repository {
    /// Builds a main repository from the given INI `properties` coming from `repos.conf`.
    pub fn build_main_repository(properties: &ini::Properties) -> Result<Self> {
        let location = Self::parse_location(properties)?;
        Self::validate_repository(&location)?;
        let categories = Self::collect_categories(&location)?;
        Self::new(location, categories, properties)
    }

    /// Builds an overlay repository from the given INI `properties` coming from `repos.conf`
    /// and main [`Repository`].
    /// Missing profiles and categories are inherited from the main repository.
    pub fn build_overlay(properties: &ini::Properties, main_repo: &Repository) -> Result<Self> {
        let location = Self::parse_location(properties)?;
        let categories = Self::collect_categories(&location)?
            .into_iter()
            .chain(main_repo.categories.iter().cloned())
            .collect::<Vec<_>>();
        Self::new(location, categories, properties)
    }

    /// Synchronizes the repository using its [`SyncHandler`].
    pub fn sync(&self) -> Result<()> {
        if let Some(sync_handler) = &self.sync_handler {
            println!("Syncing repository '{}'", self.name);
            sync_handler.sync()?;
        }
        Ok(())
    }

    /// Returns all packages in the repository that match the given `atom`.
    /// TODO: Order the returned packages by version
    /// TODO: Consider returning an iterator
    pub fn find_packages(&self, atom: &Atom) -> Vec<&Package> {
        self.packages
            .iter()
            .filter(|pkg| atom.matches(pkg))
            .collect()
    }

    /// Checks if the profile with the relative `profile_path` is valid for the given `arch`.
    /// The repository location prefix must be stripped from the passed `profile_path` string
    /// e.g.: default/linux/23.0
    pub fn is_known_profile(&self, arch: &str, profile_path: &str) -> bool {
        self.profiles_desc
            .iter()
            .any(|desc| desc.keyword == arch && desc.profile_path == profile_path)
    }

    /// Helper function to parse the repository location from the given INI `properties`.
    fn parse_location(properties: &ini::Properties) -> Result<PathBuf> {
        properties
            .get("location")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("missing location property"))
    }

    /// Builds a new [`Repository`] with the given `location` and INI `properties`.
    fn new(
        location: PathBuf,
        categories: Vec<String>,
        properties: &ini::Properties,
    ) -> Result<Self> {
        let eapi = Self::read_eapi(&location)?;
        let profiles = location.join("profiles");
        let name = Self::read_repo_name(&location)?;

        let repository = Self {
            packages: HashSet::from_iter(
                Self::collect_packages(&name, &location, &categories)
                    .with_context(|| "unable to collect packages")?,
            ),
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
            sync_handler: build_sync_handler(properties)?,
            priority: properties
                .get("priority")
                .map(|s| s.parse::<isize>())
                .transpose()
                .with_context(|| "invalid priority value")?
                .unwrap_or(0),
            name,
            location,
            eapi,
            categories,
        };
        Ok(repository)
    }

    /// Validates the main repository structure at the given `location`
    /// according to the PMS 4 specifications.
    fn validate_repository(location: &Path) -> Result<()> {
        let profiles = location.join("profiles");
        if !fs::exists(&profiles)? {
            return Err(anyhow!("Missing profiles directory"));
        }

        let required_profile_files = [
            "arch.list",
            "categories",
            "info_pkgs",
            "info_vars",
            "package.mask",
            "profiles.desc",
            "repo_name",
            "thirdpartymirrors",
            "use.desc",
            "use.local.desc",
        ];
        for file in required_profile_files {
            if profiles.join(file).is_dir() {
                return Err(anyhow!("Required file not found: profiles/{file}"));
            }
        }

        let profile_dirs = ["desc", "updates"];
        for dir in profile_dirs {
            if !fs::exists(profiles.join(dir))? {
                return Err(anyhow!("Required folder not found: profiles/{dir}"));
            }
            if !&profiles.join(dir).is_dir() {
                return Err(anyhow!("profiles/{dir} must be a directory"));
            }
        }
        Ok(())
    }

    /// Reads the repository name from the given repository path.
    fn read_repo_name(path: &Path) -> Result<String> {
        let repo_name = fs::read_to_string(path.join("profiles").join("repo_name"))?
            .lines()
            .next()
            .ok_or_else(|| anyhow!("Empty repo_name file"))?
            .to_owned();
        if !REPO_RE.is_match(&repo_name) {
            return Err(anyhow!(
                "Invalid repository name: {repo_name}. It must match the regex: {}",
                REPO_RE.as_str()
            ));
        }
        Ok(repo_name)
    }

    /// Reads the repository eapi version from the given repository `path`.
    /// Returns 0 if no eapi file exists.
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

    /// Collects all categories from the given repository `location`.
    /// Returns an empty vec if categories file doesn't exist.
    fn collect_categories(location: &Path) -> Result<Vec<String>> {
        let path = location.join("profiles").join("categories");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let categories = fs::read_to_string(&path)
            .with_context(|| anyhow!("Unable to read {}", path.display()))?
            .lines()
            .map(|line| line.to_owned())
            .collect();
        Ok(categories)
    }

    /// Collects all packages from the given repository `repo_name`, `location`.
    /// Only respects `categories` in the given list.
    fn collect_packages(
        repo_name: &str,
        location: &Path,
        categories: &[String],
    ) -> Result<Vec<Package>> {
        let mut packages = Vec::new();
        for category in categories {
            let cat_path = location.join(category);
            let entries = match fs::read_dir(&cat_path) {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            for entry in entries {
                let entry = entry?;
                if utils::is_file(&entry)? {
                    continue;
                }
                let pkg_path = entry.path();
                let pkg_name = utils::path_to_filename(&pkg_path)?;

                for file_path in utils::files_from_dir(&pkg_path)? {
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
                    let ebuild = Ebuild::from_path(file_path)?;
                    let pkg =
                        Package::new(category, pkg_name, version, repo_name)?.with_ebuild(ebuild);
                    packages.push(pkg);
                }
            }
        }
        Ok(packages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repository_regex_match() {
        let valid_names = ["gentoo", "my-repo_1", "repo123"];
        for name in valid_names {
            assert!(
                REPO_RE.is_match(name),
                "repository name '{name}' should be valid",
            );
        }
    }

    #[test]
    fn test_repository_regex_no_match() {
        let invalid_names = ["", "my repo", "repo!", "repo@123", "repo#name", "-repo"];
        for name in invalid_names {
            assert!(
                !REPO_RE.is_match(name),
                "repository name '{name}' should be invalid",
            );
        }
    }

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
