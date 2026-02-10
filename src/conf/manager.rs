use crate::deps::Atom;
use crate::linefile::LineBasedFile;
use crate::package::Package;
use crate::profile::Profile;
use crate::repository::Repository;
use crate::utils::Inherit;
use anyhow::{Context, Result};
use std::collections::HashMap;

/// Holds all package masks and should be used as the single source of truth  when checking
/// if a package is masked. Masks and unmasks are stored in a `HashMap` that maps the
/// qualified package name to a vector of [`Atom`].
#[derive(Debug)]
pub struct MaskManager {
    pub mask: HashMap<String, Vec<Atom>>,
    pub unmask: HashMap<String, Vec<Atom>>,
}

impl MaskManager {
    /// Builds a [`MaskManager`] by aggregating package masks and unmasks in the following order:
    /// 1. Repository
    /// 2. Profile
    /// 3. User defined
    pub fn new<'a>(
        repos: &mut impl Iterator<Item = &'a Repository>,
        profile: &Profile,
        user_mask: LineBasedFile,
        user_unmask: LineBasedFile,
    ) -> Result<Self> {
        let mut mask = LineBasedFile::default().inherit(&profile.package_mask);
        let mut unmask = LineBasedFile::default().inherit(&profile.package_unmask);
        for repo in repos {
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
        Ok(manager)
    }

    /// Checks if the given `package` is masked according to the current masks.
    pub fn is_masked(&self, pkg: &Package) -> bool {
        match Self::map_contains_pkg(&self.mask, pkg) {
            true => !Self::map_contains_pkg(&self.unmask, pkg),
            false => false,
        }
    }

    /// Helper function to check if a map contains a package according to its atoms.
    fn map_contains_pkg(map: &HashMap<String, Vec<Atom>>, pkg: &Package) -> bool {
        map.get(&pkg.qualified_name())
            .map(|atoms| atoms.iter().any(|atom| atom.matches(pkg)))
            .unwrap_or(false)
    }

    /// Helper function to build a map from qualified atom names to [`Atom`]
    /// from a [`LineBasedFile`].
    fn map_from_linefile(linefile: LineBasedFile) -> Result<HashMap<String, Vec<Atom>>> {
        let mut map = HashMap::new();
        for atom in linefile.into_iter().map(|line| Atom::new(&line)) {
            let atom = atom?;
            map.entry(atom.qualified_name())
                .or_insert_with(Vec::new)
                .push(atom);
        }
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::version::PackageVersion;

    #[test]
    fn test_is_masked() {
        let mask_lines = LineBasedFile::from_iter(["dev-lang/rust", "app-editors/vim"]);
        let unmask_lines = LineBasedFile::from_iter(["=dev-lang/rust-1.50"]);
        let repos = Vec::new();
        let manager = MaskManager::new(
            &mut repos.iter(),
            &Profile::default(),
            mask_lines,
            unmask_lines,
        )
        .unwrap();

        let pkg1 = Package::new(
            "dev-lang",
            "rust",
            PackageVersion::new("1.50", None, Some("2")).unwrap(),
            "gentoo",
        )
        .unwrap();
        assert!(!manager.is_masked(&pkg1), "{pkg1} should not be masked");

        let pkg2 = Package::new(
            "dev-lang",
            "rust",
            PackageVersion::new("1.60", None, Some("1")).unwrap(),
            "gentoo",
        )
        .unwrap();
        assert!(manager.is_masked(&pkg2), "{pkg2} should be masked");

        let pkg3 = Package::new(
            "app-editors",
            "vim",
            PackageVersion::new("8.2", None, None).unwrap(),
            "gentoo",
        )
        .unwrap();
        assert!(manager.is_masked(&pkg3), "{pkg3} should be masked");

        let pkg4 = Package::new(
            "app-editors",
            "nano",
            PackageVersion::new("5.0", None, None).unwrap(),
            "gentoo",
        )
        .unwrap();
        assert!(!manager.is_masked(&pkg4), "{pkg4} should not be masked");
    }
}
