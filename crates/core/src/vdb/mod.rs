pub mod package;

use crate::deps::atom::Atom;
use crate::package::cpv::CPV;
use crate::package::version::PackageVersion;
use crate::regex::PV_REV;
use crate::vdb::package::InstalledPackage;
use anyhow::{Context, Result, anyhow, bail};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use walkdir::WalkDir;

/// Regex to validate and parse `package`, `version`, `suffixes` and the `revision` from VDB.
static VDB_PKG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"^{PV_REV}$")).unwrap());

/// Represents a portage compatible VDB containing [`Package`].
pub struct Vdb {
    packages: Vec<InstalledPackage>,
}

impl Vdb {
    /// Collects and builds VDB from the given `path`.
    pub fn from_path(path: PathBuf) -> Result<Self> {
        let packages = Self::collect_packages(&path)?;
        Ok(Self { packages })
    }

    /// Returns all packages matching the given `atom`.
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
                Err(e) => bail!("unable to read category in '{}': {e}", path.display()),
            })
            .collect::<Result<Vec<_>>>()?;

        let packages = paths
            .par_iter()
            .filter_map(|p| Self::package_from_path(p).transpose())
            .collect::<Result<Vec<_>>>()?;
        Ok(packages)
    }

    /// Builds a [`Package`] from the given `path`.
    ///
    /// Returns `Ok(None)` if the path hasn't the correct syntax,
    ///   e.g. a package has an incomplete merge, indicated by the `-MERGING-` prefix.
    /// Returns `Err` if the path or metadata can't be read.
    fn package_from_path(path: &Path) -> Result<Option<InstalledPackage>> {
        let pvr = path
            .file_name()
            .and_then(|f| f.to_str())
            .with_context(|| "path contains invalid unicode")?;
        let Some(caps) = VDB_PKG_RE.captures(pvr) else {
            return Ok(None);
        };

        let category = path
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
        let pkg = InstalledPackage::new(cpv, path)
            .with_context(|| anyhow!("failed to collect package from {}", path.display()))?;
        Ok(Some(pkg))
    }
}
