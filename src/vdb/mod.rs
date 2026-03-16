pub mod package;

use crate::deps::atom::Atom;
use crate::package::cpv::CPV;
use crate::package::version::PackageVersion;
use crate::regex::PV_REV;
use crate::vdb::package::InstalledPackage;
use anyhow::{Context, Result, anyhow};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use walkdir::WalkDir;

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

    /// Collects all packages from the given VDB `path`.
    fn collect_packages(path: &Path) -> Result<Vec<InstalledPackage>> {
        let paths = WalkDir::new(path)
            .min_depth(2)
            .max_depth(2)
            .into_iter()
            .filter_entry(|e| e.file_type().is_dir())
            .map(|entry| match entry {
                Ok(entry) => Ok(entry.into_path()),
                Err(e) => Err(anyhow!(
                    "unable to read category in '{}': {e}",
                    path.display()
                )),
            });

        let mut packages = Vec::new();
        for pkg_path in paths {
            let pkg_path = pkg_path?;
            let pvr = pkg_path
                .file_name()
                .and_then(|f| f.to_str())
                .with_context(|| "path contains invalid unicode")?;
            let Some(caps) = VDB_PKG_RE.captures(pvr) else {
                continue;
            };

            let category = pkg_path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|f| f.to_str())
                .with_context(|| "path contains invalid unicode")?;
            let package = &caps["package"];
            let version = PackageVersion::new(
                &caps["version"],
                Some(&caps["suffixes"]),
                caps.name("revision").map(|m| m.as_str()),
            )?;
            let cpv = CPV::new(category, package, version)?;
            packages.push(
                InstalledPackage::new(cpv, pkg_path)
                    .with_context(|| anyhow!("unable to read package"))?,
            );
        }

        Ok(packages)
    }
}
