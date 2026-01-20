mod desc;

use crate::eapi::Eapi;
use crate::linefile::LineBasedFile;
use crate::package::Package;
use crate::package::ebuild::Ebuild;
use crate::package::version::PackageVersion;
use crate::regex::{PKG_VER_REV, REPOSITORY};
use crate::repository::desc::ProfileDescription;
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

#[derive(Debug)]
pub struct Repository {
    pub path: PathBuf,
    pub name: String,
    pub eapi: Eapi,
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
            .collect::<Vec<_>>();
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
                eapi.profile_file_dirs,
                true,
            )?,
            package_unmask: LineBasedFile::from_path(
                &path.join("profiles").join("package.unmask"),
                eapi.profile_file_dirs,
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
            .collect::<Result<_>>()?,
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
            if profiles
                .join(file)
                .metadata()
                .with_context(|| {
                    format!("Failed to read file {} from {}", file, profiles.display())
                })?
                .is_dir()
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
            .map(|line| line.to_owned())
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
                    && let Some(file_name) = entry.file_name().as_os_str().to_str()
                {
                    let pkg_path = cat_path.join(file_name);
                    for (ebuild, version) in Self::collect_package_metadata(&pkg_path, file_name)? {
                        let pkg = Package::new(category, file_name, version)?.with_ebuild(ebuild);
                        packages.insert(pkg);
                    }
                }
            }
        }
        Ok(packages)
    }

    /// Collects all ebuilds and versions for the given package directory `path` and package `name`.
    fn collect_package_metadata(path: &Path, name: &str) -> Result<Vec<(Ebuild, PackageVersion)>> {
        let mut versions = Vec::new();
        for entry in fs::read_dir(path)? {
            if let Some(entry) = entry.ok()
                && let Some(meta) = entry.metadata().ok()
                && meta.is_file()
                && let Some(file_name) = entry.file_name().into_string().ok()
                && let Some(caps) = EBUILD_RE.captures(&file_name)
                && caps["package"].starts_with(name)
            {
                let ebuild = Ebuild::from_path(entry.path()).with_context(|| {
                    anyhow!("Unable to process ebuild file {}", entry.path().display())
                })?;
                let version = PackageVersion::new(
                    &caps["version"],
                    Some(&caps["suffixes"]),
                    caps.name("revision").map(|m| m.as_str()),
                )?;
                versions.push((ebuild, version));
            }
        }
        Ok(versions)
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
