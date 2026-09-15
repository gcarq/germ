pub mod cpv;
pub mod metadata;
pub mod names;
pub mod slot;
pub mod version;

use crate::deps::atom::Atom;
use crate::package::cpv::CPV;
use crate::repository::RepoName;
use metadata::PackageMetadata;
use std::fmt;

/// Provides a trait for [`Package`] and [`InstalledPackage`] used for common operations
/// like comparison and atom matching.
pub trait PackageView {
    fn cpv(&self) -> &CPV;
    fn repo(&self) -> &RepoName;
    fn metadata(&self) -> &PackageMetadata;

    /// Returns the qualified name of the package in the format `category/name`.
    fn qualified_name(&self) -> String {
        self.cpv().qualified_name()
    }

    /// Checks if the given [`Atom`] matches.
    fn matches_atom(&self, atom: &Atom) -> bool {
        if let Some(repo) = atom.repo()
            && repo != self.repo()
        {
            return false;
        }
        // TODO: evaluate `:=` and `:SLOT=` against the slot/sub-slot saved from
        // `DEPEND` match when resolving runtime dependencies.
        if let Some(slot) = atom.slot()
            && !slot.matches(self.metadata().slot())
        {
            return false;
        }
        atom.matches(self.cpv())
    }
}

/// Represents a package within a [`Repository`] with its category, name, version and additional
/// metadata required to install it.
#[derive(Debug)]
pub struct Package {
    cpv: CPV,
    repo: RepoName,
    metadata: PackageMetadata,
}

impl Package {
    /// Creates a new [`Package`] from the given `cpv`, `repo` and `metadata`.
    pub const fn new(cpv: CPV, repo: RepoName, metadata: PackageMetadata) -> Self {
        Self {
            cpv,
            repo,
            metadata,
        }
    }
}

impl PackageView for Package {
    fn cpv(&self) -> &CPV {
        &self.cpv
    }

    fn repo(&self) -> &RepoName {
        &self.repo
    }

    fn metadata(&self) -> &PackageMetadata {
        &self.metadata
    }
}

impl fmt::Display for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.cpv, self.repo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_support::{cpv, package_metadata};
    use crate::vdb::package::InstalledPackage;

    fn assert_package_view_matches_atoms<P: PackageView>(package: &P) {
        for atom in [
            "sys-devel/gcc",
            "sys-devel/gcc::gentoo",
            "=sys-devel/gcc-15*",
            "sys-devel/gcc:15",
            "sys-devel/gcc:15=",
            "sys-devel/gcc:15/15",
            "sys-devel/gcc:15/15=",
            "sys-devel/gcc:*",
            "sys-devel/gcc:=",
        ] {
            let atom = Atom::new(atom).unwrap();
            assert!(package.matches_atom(&atom), "{atom} should match");
        }

        for atom in [
            "sys-devel/gcc::local",
            "sys-devel/binutils",
            "virtual/gcc",
            "<sys-devel/gcc-15",
            "sys-devel/gcc:14",
            "sys-devel/gcc:14=",
            "sys-devel/gcc:15/0",
            "sys-devel/gcc:15/0=",
        ] {
            let atom = Atom::new(atom).unwrap();
            assert!(!package.matches_atom(&atom), "{atom} shouldn't match");
        }
    }

    #[test]
    fn test_package_view_matches_repository_package() {
        let cpv = cpv("sys-devel", "gcc", "15.2.1_p20251122-r1");
        let repo = "gentoo".parse().unwrap();
        let package = Package::new(cpv, repo, package_metadata(&[("SLOT", "15")]));
        assert_package_view_matches_atoms(&package);
        assert_eq!(package.qualified_name(), "sys-devel/gcc");
    }

    #[test]
    fn test_package_view_matches_installed_package() {
        let package = InstalledPackage::from_parts(
            cpv("sys-devel", "gcc", "15.2.1_p20251122-r1"),
            "gentoo".parse().unwrap(),
            package_metadata(&[("SLOT", "15")]),
            Vec::default(),
        );
        assert_package_view_matches_atoms(&package);
        assert_eq!(package.qualified_name(), "sys-devel/gcc");
    }

    #[test]
    fn test_package_display() {
        let package = Package::new(
            cpv("dev-lang", "rust", "1.98.1"),
            "gentoo".parse().unwrap(),
            package_metadata(&[]),
        );
        assert_eq!(package.to_string(), "dev-lang/rust-1.98.1::gentoo");
    }
}
