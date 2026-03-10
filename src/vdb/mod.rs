pub mod package;

use crate::deps::Atom;
use crate::package::cpv::CPV;
use crate::package::version::PackageVersion;
use crate::regex::PV_REV;
use crate::utils;
use crate::vdb::package::InstalledPackage;
use anyhow::{Context, Result, anyhow};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Regex to validate and parse `package`, `version`, `suffixes` and the `revision` from VDB.
static VDB_PKG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"^{PV_REV}$")).unwrap());

/// Represents a portage compatible VDB containing [`InstalledPackage`].
pub struct Vdb {
    packages: Vec<InstalledPackage>,
}

impl Vdb {
    /// Collects and builds VDB from the given `path`.
    pub fn from_path(path: PathBuf) -> Result<Self> {
        let packages = Self::collect_packages(&path)?;
        Ok(Self { packages })
    }

    pub fn find_by_atom(&self, atom: &Atom) -> impl Iterator<Item = &InstalledPackage> {
        self.packages
            .iter()
            .filter(move |pkg| pkg.matches_atom(atom))
    }

    /// Collects categories from the given VDB `path`.
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
    fn collect_packages(path: &Path) -> Result<Vec<InstalledPackage>> {
        let categories = Self::collect_categories(path)?;
        let mut packages = Vec::new();
        for category in categories.into_iter() {
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
                let Some(caps) = VDB_PKG_RE.captures(pvr) else {
                    continue;
                };

                let name = &caps["package"];
                let version = PackageVersion::new(
                    &caps["version"],
                    Some(&caps["suffixes"]),
                    caps.name("revision").map(|m| m.as_str()),
                )?;
                let cpv = CPV::new(&category, name, version)?;
                let package = InstalledPackage::new(cpv, entry.path()).with_context(|| {
                    anyhow!("unable to read package in {}", entry.path().display())
                })?;
                packages.push(package);
            }
        }
        Ok(packages)
    }
}
