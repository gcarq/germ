use crate::package::{Package, PackageVersion, PackageVersionSuffix};
use anyhow::Result;
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

lazy_static! {
    /// Regex to validate category names according to PMS 3.1.1.
    static ref CATEGORY_NAME_RE: Regex = Regex::new(r"^[A-Za-z0-9_][A-Za-z0-9+_.-]*$").unwrap();
    /// Regex to validate and parse package versions from VDB
    static ref QUALIFIED_PACKAGE_NAME_RE: Regex = Regex::new(r"^(?<name>[A-Za-z0-9_][A-Za-z0-9+_.-]*[A-Za-z0-9+_.])-(?<version>[0-9]+(?:\.[0-9]+)*[a-z]?)(?<suffixes>(?:_(?:alpha|beta|pre|rc|p)\d*)*)(?:-r(?<revision>\d*))?$").unwrap();
}

/// Represents a portage compatible VDB containing installed packages.
pub struct Vdb {
    path: PathBuf,
    pub packages: HashSet<Package>,
}

impl Vdb {
    /// Collects and builds VDB from the given path.
    pub fn from_path(path: PathBuf) -> Result<Self> {
        let packages = Self::collect_packages(&path)?;
        Ok(Self { path, packages })
    }

    /// Collects all categories from the given VDB path.
    fn collect_categories(path: &Path) -> Result<Vec<String>> {
        let categories = fs::read_dir(path)?
            .filter_map(|entry| {
                if let Some(entry) = entry.ok()
                    && let Some(meta) = entry.metadata().ok()
                    && meta.is_dir()
                    && let Some(category) = entry.file_name().into_string().ok()
                    && CATEGORY_NAME_RE.is_match(&category)
                {
                    Some(category)
                } else {
                    None
                }
            })
            .collect();
        Ok(categories)
    }

    /// Collects all packages from the given VDB path.
    fn collect_packages(path: &Path) -> Result<HashSet<Package>> {
        let categories = Self::collect_categories(path)?;
        let mut packages = HashSet::new();
        for category in categories {
            for entry in fs::read_dir(path.join(&category))? {
                if let Some(entry) = entry.ok()
                    && let Some(meta) = entry.metadata().ok()
                    && meta.is_dir()
                    && let Some(file_name) = entry.file_name().into_string().ok()
                    && let Some(caps) = QUALIFIED_PACKAGE_NAME_RE.captures(&file_name)
                {
                    let pkg_name = caps["name"].to_string();
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
                    packages.insert(Package::new(category.clone(), pkg_name, vec![version]));
                }
            }
        }
        Ok(packages)
    }
}
