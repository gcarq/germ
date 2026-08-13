pub mod useflag;

use crate::deps::atom::Atom;
use crate::files::entry::Precedence;
use crate::files::{PackageEntries, entry::Entry};
use crate::package::PackageView;
use crate::profile::Profile;
use crate::repository::RepoPackageMasks;
use crate::types::FxHashMap;
use crate::utils::Inherit;

use log::debug;
use std::cmp::Ordering;
use std::path::Path;

/// Holds user defined package masks and unmasks,
/// usually read from `/etc/portage/package.mask`
/// and `/etc/portage/package.unmask`.
#[derive(Default)]
pub struct UserPackageMasks {
    mask: PackageEntries,
    unmask: PackageEntries,
}

impl UserPackageMasks {
    /// Builds [`UserPackageMasks`] from the given portage conf `path`.
    pub fn from_path(path: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            mask: PackageEntries::from_path(&path.join("package.mask"), Precedence::User, true)?,
            unmask: PackageEntries::from_path(
                &path.join("package.unmask"),
                Precedence::User,
                true,
            )?,
        })
    }
}

/// Holds all package masks and should be used as the single source of truth  when checking
/// if a package is masked. Masks and unmasks are stored in a `HashMap` that maps the
/// qualified package name to a vector of [`Atom`].
pub struct PackageMasks {
    mask: FxHashMap<Box<str>, Vec<Entry<Atom>>>,
    unmask: FxHashMap<Box<str>, Vec<Entry<Atom>>>,
}

impl PackageMasks {
    /// Builds a [`PackageMasks`] from repository, profile, and user definitions.
    ///
    /// Definitions are merged in the following order:
    /// 1. Repository
    /// 2. Profile
    /// 3. User defined
    pub fn new(
        repository: RepoPackageMasks,
        profile: &Profile,
        user: UserPackageMasks,
    ) -> anyhow::Result<Self> {
        let mut mask = PackageEntries::default().inherit(&profile.package_mask)?;
        let mut unmask = PackageEntries::default().inherit(&profile.package_unmask)?;
        mask.inherit_from(&repository.mask)?;
        unmask.inherit_from(&repository.unmask)?;
        mask.inherit_from(&user.mask)?;
        unmask.inherit_from(&user.unmask)?;

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
    pub fn is_masked<P: PackageView>(&self, pkg: &P) -> bool {
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
    fn find_match<'a, P: PackageView>(
        pkg: &P,
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
    use crate::package::Package;
    use crate::package::metadata::PackageMetadata;
    use crate::test_support::cpv;

    #[test]
    fn test_is_masked() {
        let mask_lines =
            PackageEntries::from_string("dev-lang/rust\napp-editors/vim".into(), Precedence::User)
                .unwrap();
        let unmask_lines =
            PackageEntries::from_string("=dev-lang/rust-1.50*".into(), Precedence::User).unwrap();
        let manager = PackageMasks::new(
            RepoPackageMasks::default(),
            &Profile::default(),
            UserPackageMasks {
                mask: mask_lines,
                unmask: unmask_lines,
            },
        )
        .unwrap();

        let repo = "gentoo".parse().unwrap();
        let cpv1 = cpv("dev-lang", "rust", "1.50-r2");
        let pkg1 = Package::new(&cpv1, &repo, PackageMetadata::default());
        assert!(!manager.is_masked(&pkg1), "{pkg1} should not be masked");

        let cpv2 = cpv("dev-lang", "rust", "1.60-r1");
        let pkg2 = Package::new(&cpv2, &repo, PackageMetadata::default());
        assert!(manager.is_masked(&pkg2), "{pkg2} should be masked");

        let cpv3 = cpv("app-editors", "vim", "8.2");
        let pkg3 = Package::new(&cpv3, &repo, PackageMetadata::default());
        assert!(manager.is_masked(&pkg3), "{pkg3} should be masked");

        let cpv4 = cpv("app-editors", "nano", "5.0");
        let pkg4 = Package::new(&cpv4, &repo, PackageMetadata::default());
        assert!(!manager.is_masked(&pkg4), "{pkg4} should not be masked");
    }
}
