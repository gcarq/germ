pub mod useflag;

use crate::deps::atom::Atom;
use crate::files::PackageEntries;
use crate::files::entry::Entry;
use crate::package::Package;
use crate::profile::Profile;
use crate::repository::set::RepoSet;
use crate::types::FxHashMap;
use crate::utils::Inherit;
use anyhow::Result;
use log::debug;
use std::cmp::Ordering;

/// Holds all package masks and should be used as the single source of truth  when checking
/// if a package is masked. Masks and unmasks are stored in a `HashMap` that maps the
/// qualified package name to a vector of [`Atom`].
pub struct PackageMasks {
    pub mask: FxHashMap<Box<str>, Vec<Entry<Atom>>>,
    pub unmask: FxHashMap<Box<str>, Vec<Entry<Atom>>>,
}

impl PackageMasks {
    /// Builds a [`PackageMasks`] by aggregating package masks and unmasks in the following order:
    /// 1. Repository
    /// 2. Profile
    /// 3. User defined
    pub fn new(
        repo_set: &RepoSet,
        profile: &Profile,
        user_mask: PackageEntries,
        user_unmask: PackageEntries,
    ) -> Result<Self> {
        let mut mask = PackageEntries::default().inherit(&profile.package_mask);
        let mut unmask = PackageEntries::default().inherit(&profile.package_unmask);
        for repo in repo_set.values() {
            mask.inherit_from(repo.package_mask()?);
            unmask.inherit_from(repo.package_unmask()?);
        }
        mask.inherit_from(&user_mask);
        unmask.inherit_from(&user_unmask);

        let mask = Self::map_from_entries(mask);
        let unmask = Self::map_from_entries(unmask);
        let manager = Self { mask, unmask };
        debug!(
            "Initialized MaskManager with {} masks and {} unmasks",
            manager.mask.len(),
            manager.unmask.len()
        );
        Ok(manager)
    }

    /// Checks if the given `package` is masked.
    pub fn is_masked(&self, pkg: &Package) -> bool {
        match Self::find_match(pkg, &self.mask) {
            Some(mask) => match Self::find_match(pkg, &self.unmask) {
                Some(unmask) => match mask.prec.cmp(&unmask.prec) {
                    Ordering::Less | Ordering::Equal => !unmask.op.as_bool(),
                    Ordering::Greater => mask.op.as_bool(),
                },
                None => true,
            },
            None => false,
        }
    }

    /// Returns the match with the highest precedence from the given `map`.
    fn find_match<'a>(
        pkg: &Package,
        map: &'a FxHashMap<Box<str>, Vec<Entry<Atom>>>,
    ) -> Option<&'a Entry<Atom>> {
        let atoms = map.get(&*pkg.qualified_name())?;
        atoms.iter().filter(|atom| pkg.matches_atom(atom)).max()
    }

    /// Helper function to build a map from qualified atom names to [`Atom`]
    /// from a [`PackageEntries`].
    fn map_from_entries(entries: PackageEntries) -> FxHashMap<Box<str>, Vec<Entry<Atom>>> {
        let mut map = FxHashMap::default();
        for atom in entries.into_iter() {
            map.entry(atom.qualified_name().into())
                .or_insert_with(|| Vec::with_capacity(1))
                .push(atom);
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::entry::Precedence;
    use crate::package::cpv::CPV;
    use crate::package::version::PackageVersion;

    #[test]
    fn test_is_masked() -> Result<()> {
        let mask_lines =
            PackageEntries::from_string("dev-lang/rust\napp-editors/vim".into(), Precedence::User)
                .unwrap();
        let unmask_lines =
            PackageEntries::from_string("=dev-lang/rust-1.50*".into(), Precedence::User).unwrap();
        let repo_set = RepoSet::default();
        let manager = PackageMasks::new(&repo_set, &Profile::default(), mask_lines, unmask_lines)?;

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
        Ok(())
    }
}
