use core::hash;
use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use anyhow::anyhow;

use crate::package::names::{CatName, PkgName};
use crate::package::parse_pf;
use crate::package::version::PackageVersion;

/// Represents a simplified form of a package only with its category, name and version.
///
/// NOTE: `fqn` holds the fully qualified name and is also used in the [`Display`] implementation
/// for performance reasons, so `category`, `package` and `version` must NOT be changed.
#[derive(Clone, Debug)]
pub struct Cpv {
    category: CatName,
    package: PkgName,
    version: PackageVersion,
    fqn: Box<str>,
}

impl Cpv {
    /// Creates a new [`Cpv`] from the given `category`, `package` and `version`.
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

impl FromStr for Cpv {
    type Err = anyhow::Error;

    /// Parses a [`Cpv`] from its fully qualified name, e.g. `app-editors/vim-9.1.1652-r2`.
    fn from_str(fqn: &str) -> anyhow::Result<Self> {
        let (category, pf) = fqn
            .split_once('/')
            .ok_or_else(|| anyhow!("invalid CPV: '{fqn}'"))?;
        let (package, version) = parse_pf(pf).ok_or_else(|| anyhow!("invalid CPV: '{fqn}'"))?;
        Ok(Self::new(category.parse()?, package, version))
    }
}

impl Eq for Cpv {}

impl PartialEq for Cpv {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Ord for Cpv {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.category, &self.package, &self.version).cmp(&(
            &other.category,
            &other.package,
            &other.version,
        ))
    }
}

impl PartialOrd for Cpv {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for Cpv {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.category.hash(state);
        self.package.hash(state);
        self.version.hash(state);
    }
}

impl fmt::Display for Cpv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.fqn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::cpv;

    #[test]
    fn test_cpv_format() {
        let vim = cpv("app-editors/vim-7.0.174-r1");
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
        let explicit_r0 = cpv("app-arch/gzip-1-r0");
        assert_eq!(explicit_r0.fqn(), "app-arch/gzip-1-r0");
        assert_eq!(explicit_r0.pf(), "gzip-1-r0");
        assert_eq!(explicit_r0.pr(), "r0");
    }

    #[test]
    fn test_cpv_from_str() {
        let cpv = "app-editors/vim-1-r1".parse::<Cpv>().unwrap();
        assert_eq!(cpv.category().as_str(), "app-editors");
        assert_eq!(cpv.package().as_str(), "vim");
        assert_eq!(cpv.version().to_string(), "1-r1");
        assert_eq!(cpv.to_string(), "app-editors/vim-1-r1");
    }

    #[test]
    fn test_cpv_from_str_err() {
        for fqn in [
            "app-editors",
            "/nano-1",
            "app-editors/nano",
            "app-editors/nano-1-2",
            "=app-editors/nano-1",
            "app-editors/nano-1::repo",
        ] {
            assert!(fqn.parse::<Cpv>().is_err(), "{fqn} should be invalid");
        }
    }
}
