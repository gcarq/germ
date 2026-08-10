pub mod cpv;
pub mod metadata;
pub mod slot;
pub mod version;

use crate::deps::atom::Atom;
use crate::package::cpv::CPV;
use crate::package::slot::PackageSlot;
use metadata::PackageMetadata;
use std::fmt;
use std::sync::Arc;

/// Provides a trait for [`Package`] and [`InstalledPackage`] used for common operations
/// like comparison and atom matching.
pub trait PackageView {
    fn cpv(&self) -> &CPV;
    fn repo(&self) -> &str;
    fn slot(&self) -> &PackageSlot;

    /// Returns the qualified name of the package in the format `category/name`.
    fn qualified_name(&self) -> String {
        self.cpv().qualified_name()
    }

    /// Checks if the given [`Atom`] matches.
    fn matches_atom(&self, atom: &Atom) -> bool {
        if let Some(repo) = atom.repo.as_deref()
            && repo != self.repo()
        {
            return false;
        }
        if let Some(slot) = &atom.slot
            && slot != self.slot()
        {
            return false;
        }
        self.cpv().matches_atom(atom)
    }
}

/// Represents a package within a [`Repository`] with its category, name, version and additional
/// metadata required to install it.
#[derive(Debug)]
pub struct Package<'r> {
    pub cpv: &'r CPV,
    pub repo: Arc<str>,
    pub metadata: PackageMetadata,
}

impl<'r> Package<'r> {
    /// Creates a new [`Package`] from the given `cpv`, `repo` and `metadata`.
    pub const fn new(cpv: &'r CPV, repo: Arc<str>, metadata: PackageMetadata) -> Self {
        Self {
            cpv,
            repo,
            metadata,
        }
    }
}

impl<'r> PackageView for Package<'r> {
    fn cpv(&self) -> &CPV {
        self.cpv
    }

    fn repo(&self) -> &str {
        &self.repo
    }

    fn slot(&self) -> &PackageSlot {
        &self.metadata.slot
    }
}

impl<'r> fmt::Display for Package<'r> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.cpv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deps::useflag::UseFlag;
    use crate::package::version::PackageVersion;
    use crate::vdb::package::InstalledPackage;

    fn assert_package_view_matches_atoms<P: PackageView>(package: &P) {
        for atom in [
            "sys-devel/gcc",
            "sys-devel/gcc::gentoo",
            "=sys-devel/gcc-15*",
            "sys-devel/gcc:15",
            "sys-devel/gcc:15=",
            "sys-devel/gcc:*",
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
        ] {
            let atom = Atom::new(atom).unwrap();
            assert!(!package.matches_atom(&atom), "{atom} shouldn't match");
        }
    }

    #[test]
    fn test_package_view_matches_repository_package() {
        let cpv = CPV::new(
            "sys-devel",
            "gcc",
            PackageVersion::try_from("15.2.1_p20251122-r1").unwrap(),
        )
        .unwrap();
        let package = Package::new(
            &cpv,
            "gentoo".into(),
            PackageMetadata {
                slot: PackageSlot::Eq("15".into()),
                ..Default::default()
            },
        );
        assert_package_view_matches_atoms(&package);
        assert_eq!(package.qualified_name(), "sys-devel/gcc");
    }

    #[test]
    fn test_package_view_matches_installed_package() {
        let package = InstalledPackage {
            cpv: CPV::new(
                "sys-devel",
                "gcc",
                PackageVersion::try_from("15.2.1_p20251122-r1").unwrap(),
            )
            .unwrap(),
            repo: "gentoo".into(),
            metadata: PackageMetadata {
                slot: PackageSlot::Eq("15".into()),
                ..Default::default()
            },
            use_flags: Vec::<UseFlag>::new(),
        };
        assert_package_view_matches_atoms(&package);
        assert_eq!(package.qualified_name(), "sys-devel/gcc");
    }

    #[test]
    fn test_package_fmt() {
        let cpv = CPV::new(
            "app-editors",
            "vim",
            PackageVersion::try_from("7.0.174-r1").unwrap(),
        )
        .unwrap();
        let package = Package::new(&cpv, "gentoo".into(), PackageMetadata::default());
        assert_eq!(package.to_string(), "app-editors/vim-7.0.174-r1");
    }
}
