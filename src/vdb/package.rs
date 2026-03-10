use crate::deps::Atom;
use crate::package::cpv::CPV;
use crate::package::slot::PackageSlot;
use anyhow::Result;
use std::path::PathBuf;
use std::str::FromStr;
use std::{fmt, fs};

#[cfg_attr(test, derive(Default))]
pub struct InstalledPackage {
    pub cpv: CPV,
    pub repo: String,
    pub slot: PackageSlot,
    path: PathBuf,
}

impl InstalledPackage {
    /// Creates a new [`InstalledPackage`] from the given `CPV`.
    ///
    /// `path` is the path to the packages vdb directory where additional metadata can be queried.
    pub fn new(cpv: CPV, path: PathBuf) -> Result<Self> {
        let repo = fs::read_to_string(path.join("repository"))?.trim().into();
        let slot = PackageSlot::from_str(fs::read_to_string(path.join("SLOT"))?.trim())?;
        Ok(Self {
            cpv,
            repo,
            slot,
            path,
        })
    }

    /// Checks if the given [`Atom`] matches this package.
    pub fn matches_atom(&self, atom: &Atom) -> bool {
        if let Some(repo_name) = &atom.repo
            && repo_name != &self.repo
        {
            return false;
        }

        if let Some(slot) = &atom.slot
            && slot != &self.slot
        {
            return false;
        }

        self.cpv.matches_atom(atom)
    }
}

impl fmt::Display for InstalledPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.cpv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::version::PackageVersion;

    #[test]
    fn test_installed_pkg_matches_atom_true() {
        let atoms = vec![
            "sys-devel/gcc",
            "sys-devel/gcc::gentoo",
            "<sys-devel/gcc-16",
            "<=sys-devel/gcc-15.2.2_p20260101",
            "sys-devel/gcc:15",
        ];
        let pkg = InstalledPackage {
            cpv: CPV::new(
                "sys-devel",
                "gcc",
                PackageVersion::new("15.2.1", Some("p20251122"), Some("1")).unwrap(),
            )
            .unwrap(),
            repo: "gentoo".into(),
            slot: PackageSlot::Eq("15".into()),
            ..Default::default()
        };
        for atom in atoms {
            let atom = Atom::new(atom).unwrap();
            assert!(pkg.matches_atom(&atom), "{atom} should match {pkg}");
        }
    }

    #[test]
    fn test_installed_pkg_matches_atom_false() {
        let atoms = vec![
            "sys-devel/gcc::local",
            "sys-devel/binutils",
            "virtual/gcc",
            "~sys-devel/gcc-15.2.1",
            "sys-devel/gcc:14",
        ];
        let pkg = InstalledPackage {
            cpv: CPV::new(
                "sys-devel",
                "gcc",
                PackageVersion::new("15.2.1", Some("p20251122"), Some("1")).unwrap(),
            )
            .unwrap(),
            repo: "gentoo".into(),
            slot: PackageSlot::Eq("15".into()),
            ..Default::default()
        };
        for atom in atoms {
            let atom = Atom::new(atom).unwrap();
            assert!(!pkg.matches_atom(&atom), "{atom} shouldn't match {pkg}");
        }
    }

    #[test]
    fn test_installed_package_fmt() {
        let pkg = InstalledPackage {
            cpv: CPV::new(
                "app-editors",
                "vim",
                PackageVersion::new("7.0.174", None, Some("1")).unwrap(),
            )
            .unwrap(),
            ..Default::default()
        };
        assert_eq!(pkg.to_string(), "app-editors/vim-7.0.174-r1");
    }
}
