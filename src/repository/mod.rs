mod desc;
mod eclass;

use crate::eapi::Eapi;
use crate::linefile::LineBasedFile;
use crate::package::Package;
use crate::package::ebuild::Ebuild;
use crate::package::version::PackageVersion;
use crate::regex::{PKG_VER_REV, REPOSITORY};
use crate::repository::desc::ProfileDescription;
use crate::repository::eclass::Eclasses;
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

#[derive(Debug)]
pub struct Repository {
    pub path: PathBuf,
    pub name: String,
    pub eapi: Eapi,
    pub categories: Vec<String>,
    pub packages: HashSet<Package>,
    pub package_mask: LineBasedFile,
    pub package_unmask: LineBasedFile,
    pub eclasses: Eclasses,
    pub arch_list: LineBasedFile,
    pub profiles_desc: Vec<ProfileDescription>,
}

impl Repository {
    /// Builds a main repository from the given path and validates its structure .
    pub fn build_main_repo_from_path(path: PathBuf) -> Result<Self> {
        Self::validate_repository(&path)?;
        let categories = Self::collect_categories(&path)?;
        Self::with_categories(path, categories)
    }

    /// Builds an overlay repository from the given path and main [`Repository`].
    /// Missing profiles files are inherited from the main repository.
    pub fn build_overlay_from_path(path: PathBuf, main_repo: &Repository) -> Result<Self> {
        let categories = Self::collect_categories(&path)?
            .into_iter()
            .chain(main_repo.categories.iter().cloned())
            .collect::<Vec<_>>();
        Self::with_categories(path, categories)
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
    fn with_categories(path: PathBuf, categories: Vec<String>) -> Result<Self> {
        let eapi = Self::read_eapi(&path)?;
        let profiles = path.join("profiles");
        Ok(Self {
            name: Self::read_repo_name(&path)?,
            packages: HashSet::from_iter(
                Self::collect_packages(&path, &categories)
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
            eclasses: Eclasses::from_path(&path.join("eclass"))
                .with_context(|| "unable to collectd eclasses")?,
            arch_list: LineBasedFile::from_path(&profiles.join("arch.list"), false, true)?,
            profiles_desc: LineBasedFile::from_path(&profiles.join("profiles.desc"), false, true)?
                .into_iter()
                .map(|line| ProfileDescription::from_line(&line))
                .collect::<Result<_>>()?,
            path,
            eapi,
            categories,
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

    /// Collects all categories from the given repository path.
    /// Returns an empty vec if categories file doesn't exist.
    fn collect_categories(path: &Path) -> Result<Vec<String>> {
        let path = path.join("profiles").join("categories");
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

    /// Collects all packages from the given repository path,
    /// only respects categories in the given list.
    fn collect_packages(path: &Path, categories: &[String]) -> Result<Vec<Package>> {
        let mut packages = Vec::new();
        for category in categories {
            let cat_path = path.join(category);
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
                    let pkg = Package::new(category, pkg_name, version)?.with_ebuild(ebuild);
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
