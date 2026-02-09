pub mod version;

use crate::ebuild::Ebuild;
use crate::package::version::PackageVersion;
use crate::regex::{CATEGORY_RE, PACKAGE};
use anyhow::{Result, anyhow};
use lazy_static::lazy_static;
use regex::Regex;
use std::{fmt, hash};

lazy_static! {
    /// Regex to validate package names.
    static ref PKG_RE: Regex = Regex::new(&format!(r"^{PACKAGE}$")).unwrap();
}

/// Represents a package with its category, name, and available versions.
/// TODO: add slot and repo information.
#[derive(Clone, Eq, Debug)]
pub struct Package {
    pub category: String,
    pub name: String,
    pub version: PackageVersion,
    pub ebuild: Option<Ebuild>,
    pub repository: String,
}

impl Package {
    /// Creates a new [`Package`] from the given `category`, `name`, and `version`.
    /// Returns `Err` if `category` or `name` are invalid according to PMS 3.1.1 and 3.1.2.
    pub fn new(
        category: &str,
        name: &str,
        version: PackageVersion,
        repository: &str,
    ) -> Result<Self> {
        if !CATEGORY_RE.is_match(category) {
            return Err(anyhow!("invalid category name: '{category}'"));
        }
        if !PKG_RE.is_match(name) {
            return Err(anyhow!("invalid package name: '{name}'"));
        }
        Ok(Self {
            category: category.to_owned(),
            name: name.to_owned(),
            version,
            ebuild: None,
            repository: repository.to_owned(),
        })
    }

    pub fn with_ebuild(mut self, ebuild: Ebuild) -> Self {
        self.ebuild = Some(ebuild);
        self
    }

    /// Returns the qualified name of the package in the format `category/name`
    /// e.g. `app-editors/vim`.
    pub fn qualified_name(&self) -> String {
        format!("{}/{}", self.category, self.name)
    }

    /// Returns the package name and version, without the revision part. For example, `vim-7.0.174`.
    pub fn p(&self) -> String {
        format!("{}-{}", self.name, self.version.pv())
    }

    /// Returns the package name, version, and revision (if any), for example `vim-7.0.174-r1`.
    pub fn pf(&self) -> String {
        format!("{}-{}", self.name, self.version.pvr())
    }

    /// Returns the package name, for example `vim`.
    pub fn pn(&self) -> String {
        self.name.clone()
    }

    /// Returns the package’s category, for example `app-editors`.
    pub fn category(&self) -> String {
        self.category.clone()
    }

    /// Returns the package version, with no revision. For example `7.0.174`.
    pub fn pv(&self) -> String {
        self.version.pv()
    }

    /// Returns the package revision, or `r0` if none exists.
    pub fn pr(&self) -> String {
        self.version.pr()
    }

    /// Returns the package version and revision (if any), for example `7.0.174` or `7.0.174-r1`.
    pub fn pvr(&self) -> String {
        self.version.pvr()
    }
}

impl PartialEq<Self> for Package {
    fn eq(&self, other: &Self) -> bool {
        self.category == other.category && self.name == other.name && self.version == other.version
    }
}

impl hash::Hash for Package {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.category.hash(state);
        self.name.hash(state);
        self.version.hash(state);
    }
}

impl fmt::Display for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.qualified_name(), self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_new_ok() {
        let package = Package::new(
            "app-editors",
            "vim",
            PackageVersion::new("1.0.0", None, None).unwrap(),
            "gentoo",
        );
        assert!(package.is_ok());
    }

    #[test]
    fn test_package_new_err() {
        let package = Package::new(
            "app-editors",
            "memtest86-",
            PackageVersion::new("1.0.0", None, None).unwrap(),
            "gentoo",
        );
        assert!(package.is_err());
    }

    #[test]
    fn test_package_fmt() {
        let package = Package::new(
            "app-editors",
            "vim",
            PackageVersion::new("7.0.174", None, Some("1")).unwrap(),
            "gentoo",
        )
        .unwrap();
        assert_eq!(package.to_string(), "app-editors/vim-7.0.174-r1");
        assert_eq!(package.qualified_name(), "app-editors/vim");
        assert_eq!(package.p(), "vim-7.0.174");
        assert_eq!(package.pf(), "vim-7.0.174-r1");
        assert_eq!(package.pn(), "vim");
        assert_eq!(package.category(), "app-editors");
        assert_eq!(package.pv(), "7.0.174");
        assert_eq!(package.pr(), "r1");
        assert_eq!(package.pvr(), "7.0.174-r1");
    }
}
