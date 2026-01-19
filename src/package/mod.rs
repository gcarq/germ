pub mod version;

use crate::package::version::PackageVersion;
use std::{fmt, hash};

/// Represents a package with its category, name, and available versions.
/// TODO: add slot and repo information.
#[derive(Eq, Debug)]
pub struct Package {
    pub category: String,
    pub name: String,
    pub version: PackageVersion,
}

impl Package {
    pub fn new(category: String, name: String, version: PackageVersion) -> Self {
        Self {
            category,
            name,
            version,
        }
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
            "app-editors".into(),
            "vim".into(),
            PackageVersion::new("1.0.0", None, 0).unwrap(),
        );
        assert_eq!(package.qualified_name(), "app-editors/vim");
    }
}
