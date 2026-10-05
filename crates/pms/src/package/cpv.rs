use core::hash;
use std::cmp::Ordering;
use std::fmt;

use crate::package::names::{CatName, PkgName};
use crate::package::version::PackageVersion;

/// Represents a simplified form of a package only with its category, name and version.
///
/// NOTE: `fqn` holds the fully qualified name and is also used in the [`Display`] implementation
/// for performance reasons, so `category`, `package` and `version` must NOT be changed.
#[derive(Clone, Debug)]
pub struct CPV {
    category: CatName,
    package: PkgName,
    version: PackageVersion,
    fqn: Box<str>,
}

impl CPV {
    /// Creates a new [`CPV`] from the given `category`, `package` and `version`.
    pub fn new(category: CatName, package: PkgName, version: PackageVersion) -> Self {
        let fqn = format!("{category}/{package}-{version}").into();
        Self {
            category,
            package,
            fqn,
            version,
        }
    }

    /// Returns the package name, e.g.: `python`.
    pub const fn package(&self) -> &PkgName {
        &self.package
    }

    /// Returns the category name, e.g.: `dev-lang`.
    pub const fn category(&self) -> &CatName {
        &self.category
    }

    /// Returns the package version, e.g.: `3.14.3-r1`.
    pub const fn version(&self) -> &PackageVersion {
        &self.version
    }

    /// Returns the fully qualified name in the format `category/package-version`
    /// e.g. `app-editors/vim-9.1.1652-r2`.
    pub fn fqn(&self) -> &str {
        &self.fqn
    }

    /// Returns the qualified name in the format `category/package`
    /// e.g. `app-editors/vim`.
    pub fn qualified_name(&self) -> String {
        format!("{}/{}", self.category(), self.package())
    }

    /// Returns the package name and version, without the revision part. For example, `vim-7.0.174`.
    pub fn p(&self) -> String {
        format!("{}-{}", self.package(), self.version.pv())
    }

    /// Returns the package name, version, and revision (if any), for example `vim-7.0.174-r1`.
    pub fn pf(&self) -> String {
        format!("{}-{}", self.package(), self.version.pvr())
    }

    /// Returns the package name, for example `vim`.
    pub fn pn(&self) -> &str {
        self.package().as_ref()
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

impl Eq for CPV {}

impl PartialEq for CPV {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Ord for CPV {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.category, &self.package, &self.version).cmp(&(
            &other.category,
            &other.package,
            &other.version,
        ))
    }
}

impl PartialOrd for CPV {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for CPV {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.category.hash(state);
        self.package.hash(state);
        self.version.hash(state);
    }
}

impl fmt::Display for CPV {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.fqn)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::cpv;

    #[test]
    fn test_cpv_format() {
        let vim = cpv("app-editors", "vim", "7.0.174-r1");
        assert_eq!(vim.to_string(), "app-editors/vim-7.0.174-r1");
        assert_eq!(vim.fqn(), "app-editors/vim-7.0.174-r1");
        assert_eq!(vim.qualified_name(), "app-editors/vim");
        assert_eq!(vim.p(), "vim-7.0.174");
        assert_eq!(vim.pf(), "vim-7.0.174-r1");
        assert_eq!(vim.pn(), "vim");
        assert_eq!(vim.pv(), "7.0.174");
        assert_eq!(vim.pr(), "r1");
        assert_eq!(vim.pvr(), "7.0.174-r1");

        // Should contain explicit r0
        let explicit_r0 = cpv("dev-libs", "pkg", "1.0-r0");
        assert_eq!(explicit_r0.fqn(), "dev-libs/pkg-1.0-r0");
        assert_eq!(explicit_r0.pf(), "pkg-1.0-r0");
        assert_eq!(explicit_r0.pr(), "r0");
    }
}
