use crate::deps::atom::Atom;
use crate::package::names::{CatName, PkgName};
use crate::package::version::PackageVersion;
use std::{cmp::Ordering, fmt};

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

    /// Checks if the given [`Atom`] matches this CPV.
    pub fn matches_atom(&self, atom: &Atom) -> bool {
        atom.category.matches(&self.category)
            && atom.package.matches(&self.package)
            && self.version.matches_atom(atom)
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
        self.package().as_str()
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

impl PartialEq for CPV {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for CPV {}

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

impl fmt::Display for CPV {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.fqn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::cpv;

    #[test]
    fn test_cpv_matches_atom_true() {
        let atoms = vec![
            "sys-devel/gcc",
            "=sys-devel/gcc-15*",
            "=sys-devel/gcc-15.2*",
            "=sys-devel/gcc-15.2.1*",
            "=sys-devel/gcc-15.2.1_p20251122-r1",
            ">sys-devel/gcc-15",
            ">=sys-devel/gcc-15.2.1",
            "<sys-devel/gcc-16",
            "<=sys-devel/gcc-15.2.2_p20260101",
            "~sys-devel/gcc-15.2.1_p20251122",
        ];
        let cpv = cpv("sys-devel", "gcc", "15.2.1_p20251122-r1");
        for atom in atoms {
            let atom = Atom::new(atom).unwrap();
            assert!(cpv.matches_atom(&atom), "{atom} should match {cpv}");
        }
    }

    #[test]
    fn test_cpv_matches_atom_false() {
        let atoms = vec![
            "sys-devel/binutils",
            "virtual/gcc",
            "<sys-devel/gcc-15",
            "<=sys-devel/gcc-15.2.1",
            ">sys-devel/gcc-16",
            ">=sys-devel/gcc-15.2.2_p20251122-r2",
            "=sys-devel/gcc-15.2.2",
            "=sys-devel/gcc-15.2.2*",
            "=sys-devel/gcc-15.2.1_p20260330",
            "~sys-devel/gcc-15.3",
            "~sys-devel/gcc-15",
            "~sys-devel/gcc-15.2",
            "~sys-devel/gcc-15.2.1",
            "~sys-devel/gcc-15.2.1_p20260101",
        ];
        let cpv = cpv("sys-devel", "gcc", "15.2.1_p20251122-r1");
        for atom in atoms {
            let atom = Atom::new(atom).unwrap();
            assert!(!cpv.matches_atom(&atom), "{atom} shouldn't match {cpv}");
        }
    }

    #[test]
    fn test_cpv_explicit_r0_formatting() {
        let cpv = cpv("dev-libs", "pkg", "1.0-r0");

        assert_eq!(cpv.fqn(), "dev-libs/pkg-1.0-r0");
        assert_eq!(cpv.to_string(), "dev-libs/pkg-1.0-r0");
        assert_eq!(cpv.pf(), "pkg-1.0-r0");
        assert_eq!(cpv.pvr(), "1.0-r0");
        assert_eq!(cpv.pr(), "r0");
    }

    #[test]
    fn test_package_fmt() {
        let cpv = cpv("app-editors", "vim", "7.0.174-r1");
        assert_eq!(cpv.to_string(), "app-editors/vim-7.0.174-r1");
        assert_eq!(cpv.fqn(), "app-editors/vim-7.0.174-r1");
        assert_eq!(cpv.qualified_name(), "app-editors/vim");
        assert_eq!(cpv.p(), "vim-7.0.174");
        assert_eq!(cpv.pf(), "vim-7.0.174-r1");
        assert_eq!(cpv.pn(), "vim");
        assert_eq!(cpv.pv(), "7.0.174");
        assert_eq!(cpv.pr(), "r1");
        assert_eq!(cpv.pvr(), "7.0.174-r1");
    }
}
