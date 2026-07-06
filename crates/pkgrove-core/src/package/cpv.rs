use crate::deps::atom::{Atom, AtomIdent};
use crate::ebuild::Ebuild;
use crate::ebuild::handler::{EbuildPhase, EbuildPhaseHandler};
use crate::makenv::MakeEnv;
use crate::package::metadata::PackageMetadata;
use crate::package::version::PackageVersion;
use crate::regex::{CATEGORY_RE, PKG_RE};
use crate::repository::Repository;
use crate::types::FxHashMap;
use anyhow::{Context, Result, anyhow};
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::path::Path;

/// Represents a simplified form of a package only with its category, name and version.
///
/// NOTE: `fqn` holds the fully qualified name and is also used in the [`Display`] implementation
/// for performance reasons, so `category`, `package` and `version` must NOT be changed.
///
/// TODO: consider adding a `new_unchecked` constructor, that also passes the `fqn` that doesn't
///  validate for performance reasons
#[derive(Archive, Serialize, Deserialize, Clone, Ord, PartialOrd, Eq, PartialEq, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct CPV {
    category: Box<str>,
    package: Box<str>,
    version: PackageVersion,
    fqn: Box<str>,
}

impl CPV {
    /// Creates a new [`CPV`] from the given `category`, `package` and `version`.
    pub fn new(category: &str, package: &str, version: PackageVersion) -> Result<Self> {
        if !CATEGORY_RE.is_match(category) {
            return Err(anyhow!("invalid category name: '{category}'"));
        }
        if !PKG_RE.is_match(package) {
            return Err(anyhow!("invalid package name: '{package}'"));
        }
        Ok(Self::new_unchecked(category, package, version))
    }

    /// Creates a new [`CPV`] without validating `category` or `package`.
    pub fn new_unchecked(category: &str, package: &str, version: PackageVersion) -> Self {
        Self {
            category: category.into(),
            package: package.into(),
            fqn: format!("{category}/{package}-{version}").into(),
            version,
        }
    }

    /// Checks if the given [`Atom`] matches this CPV.
    pub fn matches_atom(&self, atom: &Atom) -> bool {
        if let AtomIdent::Exact(category) = &atom.category
            && *self.category != *category
        {
            return false;
        }
        if let AtomIdent::Exact(package) = &atom.package
            && *self.package != *package
        {
            return false;
        }
        self.version.matches_atom(atom)
    }

    /// Generates and returns the [`PackageMetadata`] for this CPV.
    ///
    /// The `repo` is needed during the `depend` phase.
    ///
    /// Returns an `Err` if the ebuild can't be resolved or metadata is missing.
    pub fn generate_metadata(
        &self,
        ebuild_path: &Path,
        repo: &Repository,
    ) -> Result<PackageMetadata> {
        let ebuild = Ebuild::new(ebuild_path.to_path_buf(), self, repo)
            .with_context(|| anyhow!("unable to create ebuild from '{}'", ebuild_path.display()))?;
        let mut handler =
            EbuildPhaseHandler::new(&ebuild, EbuildPhase::Depend, &MakeEnv::default());
        let data = handler
            .spawn()
            .with_context(|| "ebuild script execution failed")?;
        let data = data
            .iter()
            .filter_map(|d| d.split_once('=').map(|(k, v)| (k.trim(), v.trim())))
            .collect::<FxHashMap<_, _>>();

        PackageMetadata::from_map(data)
            .with_context(|| anyhow!("unable to create metadata from ebuild output"))
    }

    /// Returns the category of the package, e.g.: `dev-lang`.
    pub fn package(&self) -> &str {
        &self.package
    }

    /// Returns the package name, e.g.: `python`.
    pub fn category(&self) -> &str {
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
        format!("{}/{}", self.category, self.package)
    }

    /// Returns the package name and version, without the revision part. For example, `vim-7.0.174`.
    pub fn p(&self) -> String {
        format!("{}-{}", self.package, self.version.pv())
    }

    /// Returns the package name, version, and revision (if any), for example `vim-7.0.174-r1`.
    pub fn pf(&self) -> String {
        format!("{}-{}", self.package, self.version)
    }

    /// Returns the package name, for example `vim`.
    pub fn pn(&self) -> String {
        self.package.clone().into()
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
        self.version.to_string()
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

    #[test]
    fn test_cpv_new_ok() {
        let cpv = CPV::new(
            "dev-lang",
            "R",
            PackageVersion::new("4.5.2", None, None).unwrap(),
        );
        assert!(cpv.is_ok());
    }

    #[test]
    fn test_cpv_new_err() {
        let cpv = CPV::new(
            "app-editors",
            "memtest86-",
            PackageVersion::new("1.0.0", None, None).unwrap(),
        );
        assert!(cpv.is_err());
    }

    #[test]
    fn test_cpv_new_unchecked() {
        let cpv = CPV::new_unchecked(
            "app-editors",
            "vim",
            PackageVersion::new("9.1.1652", None, Some("2")).unwrap(),
        );
        assert_eq!(cpv.to_string(), "app-editors/vim-9.1.1652-r2");
    }

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
        let cpv = CPV::new(
            "sys-devel",
            "gcc",
            PackageVersion::new("15.2.1", Some("p20251122"), Some("1")).unwrap(),
        )
        .unwrap();
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
        let cpv = CPV::new(
            "sys-devel",
            "gcc",
            PackageVersion::new("15.2.1", Some("p20251122"), Some("1")).unwrap(),
        )
        .unwrap();
        for atom in atoms {
            let atom = Atom::new(atom).unwrap();
            assert!(!cpv.matches_atom(&atom), "{atom} shouldn't match {cpv}");
        }
    }

    #[test]
    fn test_package_fmt() {
        let cpv = CPV::new(
            "app-editors",
            "vim",
            PackageVersion::new("7.0.174", None, Some("1")).unwrap(),
        )
        .unwrap();
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
