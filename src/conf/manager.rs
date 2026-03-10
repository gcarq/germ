use crate::deps::Atom;
use crate::linefile::LineBasedFile;
use crate::package::Package;
use crate::profile::Profile;
use crate::repository::set::RepoSet;
use crate::utils::Inherit;
use anyhow::{Context, Result};
use log::debug;
use std::collections::HashMap;

/// Holds all package masks and should be used as the single source of truth  when checking
/// if a package is masked. Masks and unmasks are stored in a `HashMap` that maps the
/// qualified package name to a vector of [`Atom`].
pub struct MaskManager {
    pub mask: HashMap<Box<str>, Vec<Atom>>,
    pub unmask: HashMap<Box<str>, Vec<Atom>>,
}

impl MaskManager {
    /// Builds a [`MaskManager`] by aggregating package masks and unmasks in the following order:
    /// 1. Repository
    /// 2. Profile
    /// 3. User defined
    pub fn new(
        repo_set: &RepoSet,
        profile: &Profile,
        user_mask: LineBasedFile,
        user_unmask: LineBasedFile,
    ) -> Result<Self> {
        let mut mask = LineBasedFile::default().inherit(&profile.package_mask);
        let mut unmask = LineBasedFile::default().inherit(&profile.package_unmask);
        for repo in repo_set.values() {
            mask.inherit_from(&repo.package_mask);
            unmask.inherit_from(&repo.package_unmask);
        }
        mask.inherit_from(&user_mask);
        unmask.inherit_from(&user_unmask);

        let mask =
            Self::map_from_linefile(mask).with_context(|| "unable to collect package masks")?;
        let unmask =
            Self::map_from_linefile(unmask).with_context(|| "unable to collect package unmasks")?;
        let manager = Self { mask, unmask };
        debug!(
            "Initialized MaskManager with {} masks and {} unmasks",
            manager.mask.len(),
            manager.unmask.len()
        );
        Ok(manager)
    }

    /// Checks if the given `package` is masked according to the current masks.
    pub fn is_masked(&self, pkg: &Package) -> bool {
        match Self::map_contains_pkg(&self.mask, pkg) {
            true => !Self::map_contains_pkg(&self.unmask, pkg),
            false => false,
        }
    }

    /// Helper function to check if the given `map` contains a package according to its atoms.
    fn map_contains_pkg(map: &HashMap<Box<str>, Vec<Atom>>, pkg: &Package) -> bool {
        map.get(&*pkg.qualified_name())
            .is_some_and(|atoms| atoms.iter().any(|atom| pkg.matches_atom(atom)))
    }

    /// Helper function to build a map from qualified atom names to [`Atom`]
    /// from a [`LineBasedFile`].
    fn map_from_linefile(linefile: LineBasedFile) -> Result<HashMap<Box<str>, Vec<Atom>>> {
        let mut map = HashMap::new();
        for atom in linefile.into_iter().map(|line| Atom::new(&line)) {
            let atom = atom?;
            map.entry(atom.qualified_name().into())
                .or_insert_with(|| Vec::with_capacity(1))
                .push(atom);
        }
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::cpv::CPV;
    use crate::package::version::PackageVersion;

    #[test]
    fn test_is_masked() {
        let mask_lines = LineBasedFile::from_iter(["dev-lang/rust", "app-editors/vim"]);
        let unmask_lines = LineBasedFile::from_iter(["=dev-lang/rust-1.50*"]);
        let repo_set = RepoSet::default();
        let manager =
            MaskManager::new(&repo_set, &Profile::default(), mask_lines, unmask_lines).unwrap();

        let pkg1 = Package {
            cpv: CPV::new(
                "dev-lang",
                "rust",
                PackageVersion::new("1.50", None, Some("2")).unwrap(),
            )
            .unwrap(),
            ..Default::default()
        };
        assert!(!manager.is_masked(&pkg1), "{pkg1} should not be masked");

        let pkg2 = Package {
            cpv: CPV::new(
                "dev-lang",
                "rust",
                PackageVersion::new("1.60", None, Some("1")).unwrap(),
            )
            .unwrap(),
            ..Default::default()
        };
        assert!(manager.is_masked(&pkg2), "{pkg2} should be masked");

        let pkg3 = Package {
            cpv: CPV::new(
                "app-editors",
                "vim",
                PackageVersion::new("8.2", None, None).unwrap(),
            )
            .unwrap(),
            ..Default::default()
        };
        assert!(manager.is_masked(&pkg3), "{pkg3} should be masked");

        let pkg4 = Package {
            cpv: CPV::new(
                "app-editors",
                "nano",
                PackageVersion::new("5.0", None, None).unwrap(),
            )
            .unwrap(),
            ..Default::default()
        };
        assert!(!manager.is_masked(&pkg4), "{pkg4} should not be masked");
    }
}
