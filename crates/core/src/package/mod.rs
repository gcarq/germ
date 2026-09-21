pub mod cpv;
pub mod metadata;
pub mod names;
mod requirement;
pub mod slot;
pub mod version;

pub use requirement::AtomRequirement;

use crate::repository::RepoName;
use crate::{atom::Atom, package::cpv::CPV};
use metadata::PackageMetadata;
use std::{fmt, hash};

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
#[derive(Clone, Debug)]
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

impl Eq for Package {}

impl PartialEq for Package {
    fn eq(&self, other: &Self) -> bool {
        self.cpv == other.cpv && self.repo == other.repo
    }
}

impl hash::Hash for Package {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.cpv.hash(state);
        self.repo.hash(state);
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
    use crate::test_support::{cpv, pkg_metadata, pkg};
    use crate::vdb::package::InstalledPackage;

    fn assert_package_view_matches_atoms<P: PackageView>(pkg: &P) {
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
            assert!(pkg.matches_atom(&atom), "{atom} should match");
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
            assert!(!pkg.matches_atom(&atom), "{atom} shouldn't match");
        }
    }

    #[test]
    fn test_package_view_matches() {
        let cpv = cpv("sys-devel", "gcc", "15.2.1_p20251122-r1");
        let repo: RepoName = "gentoo".parse().unwrap();

        let pkg = Package::new(
            cpv.clone(),
            repo.clone(),
            pkg_metadata(&[("SLOT", "15")]),
        );
        assert_package_view_matches_atoms(&pkg);
        assert_eq!(pkg.qualified_name(), "sys-devel/gcc");

        let installed = InstalledPackage::from_parts(
            cpv,
            repo,
            pkg_metadata(&[("SLOT", "15")]),
            Vec::default(),
        );
        assert_package_view_matches_atoms(&installed);
        assert_eq!(installed.qualified_name(), "sys-devel/gcc");
    }

    #[test]
    fn test_package_display() {
        let pkg = pkg("dev-lang", "rust", "1.98.1", &[]);
        assert_eq!(pkg.to_string(), "dev-lang/rust-1.98.1::gentoo");
    }
}
