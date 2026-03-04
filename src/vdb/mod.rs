use crate::package::Package;
use crate::package::version::PackageVersion;
use crate::regex::PKG_VER_REV;
use crate::utils;
use anyhow::{Context, Result, anyhow};
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Regex to validate and parse `package`, `version`, `suffixes` and the `revision` from VDB.
static PKG_VER_REV_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^{PKG_VER_REV}$")).unwrap());

/// Represents a portage compatible VDB containing installed packages.
pub struct Vdb {
    path: PathBuf,
    pub packages: HashSet<Package>,
}

impl Vdb {
    /// Collects and builds VDB from the given `path`.
    pub fn from_path(path: PathBuf) -> Result<Self> {
        let packages = Self::collect_packages(&path)?;
        Ok(Self { path, packages })
    }

    /// Collects all categories from the given VDB `path`.
    fn collect_categories(path: &Path) -> Result<Vec<String>> {
        fs::read_dir(path)?
            .filter_map(|entry| match entry {
                Ok(entry) => match utils::is_file(&entry) {
                    Ok(true) => None,
                    Ok(false) => Some(
                        entry
                            .file_name()
                            .into_string()
                            .map_err(|_| anyhow!("unicode error")),
                    ),
                    Err(err) => Some(Err(anyhow!(err))),
                },
                Err(err) => Some(Err(anyhow!(err))),
            })
            .collect()
    }

    /// Collects all packages from the given VDB `path`.
    fn collect_packages(path: &Path) -> Result<HashSet<Package>> {
        let categories = Self::collect_categories(path)?;
        let mut packages = HashSet::new();
        for category in categories {
            for entry in fs::read_dir(path.join(&category))? {
                let entry = entry?;
                if utils::is_file(&entry)? {
                    continue;
                }
                let file_name = entry.file_name();
                let pvr = file_name
                    .as_os_str()
                    .to_str()
                    .with_context(|| "path contains invalid unicode")?;
                let Some(caps) = PKG_VER_REV_RE.captures(pvr) else {
                    continue;
                };

                let version = PackageVersion::new(
                    &caps["version"],
                    Some(&caps["suffixes"]),
                    caps.name("revision").map(|m| m.as_str()),
                )?;
                let repo = fs::read_to_string(entry.path().join("repository"))?;
                let package = Package::new(&category, &caps["package"], version, &repo)?;
                packages.insert(package);
            }
        }
        Ok(packages)
    }
}
