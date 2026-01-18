mod desc;

use crate::consts::SUPPORTED_EAPI;
use crate::linefile::LineBasedFile;
use crate::package::Package;
use crate::package::version::{PackageVersion, PackageVersionSuffix};
use crate::repository::desc::ProfileDescription;
use crate::utils::FileFromPath;
use anyhow::{Context, Result, anyhow};
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

lazy_static! {
    /// TODO: Rethink placement of those regexes. The ones in package.atom should be used instead.
    /// Regex to validate repository names according to PMS 3.1.5.
    static ref REPOSITORY_NAME_RE: Regex = Regex::new(r"^[A-Za-z0-9_-]*[A-Za-z0-9_]$").unwrap();
    /// Regex to validate category names according to PMS 3.1.1.
    static ref CATEGORY_NAME_RE: Regex = Regex::new(r"^[A-Za-z0-9_][A-Za-z0-9+_.-]*$").unwrap();
    /// Regex to validate package names according to PMS 3.1.2.
    static ref PACKAGE_NAME_RE: Regex = Regex::new(r"^[A-Za-z0-9_][A-Za-z0-9+_.-]*[A-Za-z0-9+_.]$").unwrap();
    /// Regex to validate and parse package versions according to PMS 3.2.
    static ref PACKAGE_VERSION_RE: Regex = Regex::new(r"^(?<name>[A-Za-z0-9_][A-Za-z0-9+_.-]*[A-Za-z0-9+_.])-(?<version>[0-9]+(?:\.[0-9]+)*[a-z]?)(?<suffixes>(?:_(?:alpha|beta|pre|rc|p)\d*)*)(?:-r(?<revision>\d*))?.ebuild$").unwrap();
}

#[derive(Debug)]
pub struct Repository {
    pub path: PathBuf,
    pub name: String,
    pub eapi: usize,
    pub packages: HashSet<Package>,
    pub package_mask: LineBasedFile,
    pub package_unmask: LineBasedFile,
    pub arch_list: LineBasedFile,
    pub profiles_desc: Vec<ProfileDescription>,
}

impl Repository {
    /// Builds a main repository from the given path and validates its structure .
    pub fn build_main_repo_from_path(path: PathBuf) -> Result<Self> {
        Self::validate_repository(&path)?;
        let categories = Self::collect_categories(&path)?;
        Self::with_categories(path, &categories)
    }

    /// Builds an overlay repository from the given path and main [`Repository`].
    /// Missing profiles files are inherited from the main repository.
    pub fn build_overlay_from_path(path: PathBuf, main_repo: &Repository) -> Result<Self> {
        let categories = Self::collect_categories(&path)?
            .into_iter()
            .chain(Self::collect_categories(&main_repo.path)?)
            .collect::<Vec<String>>();
        Self::with_categories(path, &categories)
    }

    /// Checks if the profile with the relative `profile_path` is valid for the given `arch`.
    /// The repository location prefix must be stripped from the passed `profile_path` string
    /// e.g.: default/linux/23.0
    pub fn is_known_profile(&self, arch: &str, profile_path: &str) -> bool {
        self.profiles_desc
            .iter()
            .any(|desc| desc.keyword == arch && desc.profile_path == profile_path)
    }

    /// Builds a new [`Repository`] with the given categories.
    fn with_categories(path: PathBuf, categories: &[String]) -> Result<Self> {
        let eapi = Self::read_eapi(&path)?;
        Ok(Self {
            name: Self::read_repo_name(&path)?,
            packages: Self::collect_packages(&path, categories)?,
            package_mask: LineBasedFile::from_path(
                &path.join("profiles").join("package.mask"),
                eapi > 6,
                true,
            )?,
            package_unmask: LineBasedFile::from_path(
                &path.join("profiles").join("package.unmask"),
                eapi > 6,
                true,
            )?,
            arch_list: LineBasedFile::from_path(
                &path.join("profiles").join("arch.list"),
                false,
                true,
            )?,
            profiles_desc: LineBasedFile::from_path(
                &path.join("profiles").join("profiles.desc"),
                false,
                true,
            )?
            .iter()
            .map(|line| ProfileDescription::from_line(line))
            .collect::<Result<Vec<ProfileDescription>>>()?,
            path,
            eapi,
        })
    }

    /// Validates the main repository structure at the given path
    /// according to the PMS 4 specifications.
    fn validate_repository(path: &Path) -> Result<()> {
        let profiles = path.join("profiles");
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
            if !&profiles
                .join(file)
                .metadata()
                .with_context(|| {
                    format!("Failed to read file {} from {}", file, profiles.display())
                })?
                .is_file()
            {
                return Err(anyhow!("Required file not found: profiles/{file}"));
            }
        }

        let profile_dirs = ["desc", "updates"];
        for dir in profile_dirs {
            if !fs::exists(profiles.join(dir))? {
                return Err(anyhow!("Required folder not found: profiles/{dir}"));
            }
            if !&profiles.join(dir).metadata()?.is_dir() {
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
            .ok_or(anyhow!("Empty repo_name file"))?
            .to_string();
        if !REPOSITORY_NAME_RE.is_match(&repo_name) {
            return Err(anyhow!(
                "Invalid repository name: {repo_name}. It must match the regex: {}",
                REPOSITORY_NAME_RE.as_str()
            ));
        }
        Ok(repo_name)
    }

    /// Reads the repository eapi version from the given repository path.
    /// Returns 0 if no eapi file exists.
    fn read_eapi(path: &Path) -> Result<usize> {
        let eapi_file = path.join("profiles").join("eapi");
        if !fs::exists(&eapi_file)? {
            return Ok(0);
        }
        let eapi = fs::read_to_string(&eapi_file)?
            .lines()
            .next()
            .ok_or(anyhow!("Empty eapi file"))?
            .parse::<usize>()
            .context("eapi version must be an unsigned integer")?;
        if eapi > SUPPORTED_EAPI {
            return Err(anyhow!(
                "Unsupported eapi version: {eapi}. Supported versions are 0 to {SUPPORTED_EAPI}"
            ));
        }
        Ok(eapi)
    }

    /// Collects all categories from the given repository path.
    /// Returns an empty vec if categories file doesn't exist.
    fn collect_categories(path: &Path) -> Result<Vec<String>> {
        let path = path.join("profiles").join("categories");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let categories = fs::read_to_string(&path)
            .with_context(|| format!("Unable to read {}", path.display()))?
            .lines()
            .filter(|&line| CATEGORY_NAME_RE.is_match(line))
            .map(|line| line.to_string())
            .collect();
        Ok(categories)
    }

    /// Collects all packages from the given repository path,
    /// only respects categories in the given list.
    fn collect_packages(path: &Path, categories: &[String]) -> Result<HashSet<Package>> {
        let mut packages = HashSet::new();
        for category in categories {
            let cat_path = path.join(category);
            if !cat_path.exists() {
                continue;
            }
            for entry in fs::read_dir(&cat_path)? {
                if let Some(entry) = entry.ok()
                    && let Some(meta) = entry.metadata().ok()
                    && meta.is_dir()
                    && let Some(file_name) = entry.file_name().into_string().ok()
                    && PACKAGE_NAME_RE.is_match(&file_name)
                {
                    let pkg_path = cat_path.join(&file_name);
                    for version in Self::collect_package_versions(&pkg_path, &file_name)? {
                        packages.insert(Package::new(category.clone(), file_name.clone(), version));
                    }
                }
            }
        }
        Ok(packages)
    }

    /// Collects all versions for the given package directory, package name and regex.
    fn collect_package_versions(path: &Path, name: &str) -> Result<Vec<PackageVersion>> {
        let mut versions = Vec::new();
        for entry in fs::read_dir(path)? {
            if let Some(entry) = entry.ok()
                && let Some(meta) = entry.metadata().ok()
                && meta.is_file()
                && let Some(file_name) = entry.file_name().into_string().ok()
                && let Some(caps) = PACKAGE_VERSION_RE.captures(&file_name)
                && caps["name"].starts_with(name)
            {
                let suffixes = caps["suffixes"]
                    .split('_')
                    .filter(|s| !s.is_empty())
                    .map(PackageVersionSuffix::new)
                    .collect();
                let version = PackageVersion::new(
                    caps["version"].to_string(),
                    suffixes,
                    caps.name("revision")
                        .and_then(|r| r.as_str().parse::<usize>().ok())
                        .unwrap_or(0),
                );
                versions.push(version);
            }
        }
        Ok(versions)
    }
}
