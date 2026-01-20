pub mod ebuild;
pub mod version;

use crate::package::ebuild::Ebuild;
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
#[derive(Eq, Debug)]
pub struct Package {
    pub category: String,
    pub name: String,
    pub version: PackageVersion,
    pub ebuild: Option<Ebuild>,
}

impl Package {
    /// Creates a new [`Package`] from the given `category`, `name`, and `version`.
    /// Returns `Err` if `category` or `name` are invalid according to PMS 3.1.1 and 3.1.2.
    pub fn new(category: &str, name: &str, version: PackageVersion) -> Result<Self> {
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
        })
    }

    pub fn with_ebuild(mut self, ebuild: Ebuild) -> Self {
        self.ebuild = Some(ebuild);
        self
    }

    /// Returns the qualified name of the package in the format category/name e.g. app-editors/vim.
    pub fn qualified_name(&self) -> String {
        format!("{}/{}", self.category, self.name)
    }
}

impl PartialEq<Self> for Package {
    fn eq(&self, other: &Self) -> bool {
        self.qualified_name() == other.qualified_name()
    }
}

impl hash::Hash for Package {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.qualified_name().hash(state);
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
    fn test_package_qualified_name() {
        let package = Package::new(
            "app-editors",
            "vim",
            PackageVersion::new("1.0.0", None, None).unwrap(),
        )
        .unwrap();
        assert_eq!(package.qualified_name(), "app-editors/vim");
    }

    #[test]
    fn test_package_new_ok() {
        let package = Package::new(
            "app-editors",
            "vim",
            PackageVersion::new("1.0.0", None, None).unwrap(),
        );
        assert!(package.is_ok());
    }

    #[test]
    fn test_package_new_err() {
        let package = Package::new(
            "app-editors",
            "memtest86-",
            PackageVersion::new("1.0.0", None, None).unwrap(),
        );
        assert!(package.is_err());
    }
}
