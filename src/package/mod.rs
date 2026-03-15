pub mod cpv;
pub mod metadata;
pub mod slot;
pub mod version;

use crate::deps::atom::Atom;
use crate::package::cpv::CPV;
use metadata::PackageMetadata;
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;

/// Represents a package within a [`Repository`] with its category, name, version and additional
/// metadata required to install it.
#[derive(Archive, Serialize, Deserialize)]
#[cfg_attr(test, derive(Default, Debug))]
pub struct Package {
    pub cpv: CPV,
    pub repo: String,
    pub metadata: PackageMetadata,
}

impl Package {
    /// Creates a new [`Package`] from the given `cpv`, `repo` and `metadata`.
    ///
    /// Returns `Err` if `category` or `name` are invalid according to PMS 3.1.1 and 3.1.2.
    pub const fn new(cpv: CPV, repo: String, metadata: PackageMetadata) -> Self {
        Self {
            cpv,
            repo,
            metadata,
        }
    }

    /// Checks if the given [`Atom`] matches this package.
    pub fn matches_atom(&self, atom: &Atom) -> bool {
        if let Some(repo_name) = &atom.repo
            && repo_name != &self.repo
        {
            return false;
        }
        if let Some(slot) = &atom.slot
            && slot != &self.metadata.slot
        {
            return false;
        }
        self.cpv.matches_atom(atom)
    }

    /// Returns the qualified name of the package in the format `category/name`
    /// e.g. `app-editors/vim`.
    pub fn qualified_name(&self) -> String {
        self.cpv.qualified_name()
    }
}

impl fmt::Display for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.cpv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::slot::PackageSlot;
    use crate::package::version::PackageVersion;

    #[test]
    fn test_pkg_matches_atom_true() {
        let atoms = vec![
            "sys-devel/gcc",
            "sys-devel/gcc::gentoo",
            "=sys-devel/gcc-15*",
            "sys-devel/gcc:15",
            "sys-devel/gcc:15=",
            "sys-devel/gcc:*",
        ];
        let pkg = Package {
            cpv: CPV::new(
                "sys-devel",
                "gcc",
                PackageVersion::new("15.2.1", Some("p20251122"), Some("1")).unwrap(),
            )
            .unwrap(),
            repo: "gentoo".into(),
            metadata: PackageMetadata {
                slot: PackageSlot::Eq("15".into()),
                ..Default::default()
            },
        };
        for atom in atoms {
            let atom = Atom::new(atom).unwrap();
            assert!(pkg.matches_atom(&atom), "{atom} should match {pkg}");
        }
    }

    #[test]
    fn test_pkg_matches_atom_false() {
        let atoms = vec![
            "sys-devel/gcc::local",
            "sys-devel/binutils",
            "virtual/gcc",
            "<sys-devel/gcc-15",
            "sys-devel/gcc:14",
            "sys-devel/gcc:14=",
            "sys-devel/gcc:15/0",
        ];
        let pkg = Package {
            cpv: CPV::new(
                "sys-devel",
                "gcc",
                PackageVersion::new("15.2.1", Some("p20251122"), Some("1")).unwrap(),
            )
            .unwrap(),
            repo: "gentoo".into(),
            metadata: PackageMetadata {
                slot: PackageSlot::Eq("15".into()),
                ..Default::default()
            },
        };
        for atom in atoms {
            let atom = Atom::new(atom).unwrap();
            assert!(!pkg.matches_atom(&atom), "{atom} shouldn't match {pkg}");
        }
    }

    #[test]
    fn test_package_fmt() {
        let package = Package {
            cpv: CPV::new(
                "app-editors",
                "vim",
                PackageVersion::new("7.0.174", None, Some("1")).unwrap(),
            )
            .unwrap(),
            ..Default::default()
        };
        assert_eq!(package.to_string(), "app-editors/vim-7.0.174-r1");
        assert_eq!(package.qualified_name(), "app-editors/vim");
    }
}
