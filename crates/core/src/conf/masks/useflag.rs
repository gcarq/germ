use crate::deps::atom::Atom;
use crate::files::UseEntries;
use crate::files::entry::Entry;
use crate::files::pkguse::{EntryUseFlags, PackageUseEntries};
use crate::package::PackageView;
use crate::profile::Profile;
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::UseFlag;
use crate::utils::Inherit;

/// This struct is the only truth whether a USE flag is masked or forced.
/// TODO:
///   * improve data structure
pub struct UseMasks {
    // Masked USE flags
    use_mask: FxHashSet<UseFlag>,
    // Forced USE flags
    use_force: FxHashSet<UseFlag>,
    // Same as above but for merged packages due to a stable keyword
    use_stable_mask: FxHashSet<UseFlag>,
    use_stable_force: FxHashSet<UseFlag>,

    // Enabled USE flags on a per-package basis
    #[allow(unused)]
    package_use: FxHashMap<Atom, EntryUseFlags>,
    // Masked USE flags on a per-package basis
    package_use_mask: FxHashMap<Atom, EntryUseFlags>,
    // Forced USE flags on a per-package basis
    package_use_force: FxHashMap<Atom, EntryUseFlags>,
    // Same as above but for merged packages due to a stable keyword
    package_use_stable_mask: FxHashMap<Atom, EntryUseFlags>,
    package_use_stable_force: FxHashMap<Atom, EntryUseFlags>,
}

impl UseMasks {
    pub fn new(
        profile: &Profile,
        package_use: PackageUseEntries,
        use_mask: UseEntries,
        package_use_mask: PackageUseEntries,
    ) -> anyhow::Result<Self> {
        let use_mask = use_mask
            .inherit(&profile.use_mask)?
            .into_iter()
            .map(Entry::into_inner)
            .collect::<FxHashSet<_>>();
        let use_force = profile
            .use_force
            .iter()
            .map(|flag| flag.clone().into_inner())
            .collect::<FxHashSet<_>>();
        let use_stable_mask = profile
            .use_stable_mask
            .iter()
            .map(|flag| flag.clone().into_inner())
            .collect::<FxHashSet<_>>();
        let use_stable_force = profile
            .use_stable_force
            .iter()
            .map(|flag| flag.clone().into_inner())
            .collect::<FxHashSet<_>>();

        let package_use = package_use.inherit(&profile.package_use)?.into_inner();

        let package_use_mask = package_use_mask
            .inherit(&profile.package_use_mask)?
            .into_inner();
        let package_use_force = profile.package_use_force.clone().into_inner();
        let package_use_stable_mask = profile.package_use_stable_mask.clone().into_inner();
        let package_use_stable_force = profile.package_use_stable_force.clone().into_inner();

        Ok(Self {
            use_mask,
            use_force,
            use_stable_mask,
            use_stable_force,
            package_use,
            package_use_mask,
            package_use_force,
            package_use_stable_mask,
            package_use_stable_force,
        })
    }

    /// Checks if the given [`UseFlag`] is masked.
    pub fn is_masked(&self, flag: &UseFlag) -> bool {
        self.use_mask.contains(flag) || self.use_stable_mask.contains(flag)
    }

    /// Checks if the given [`UseFlag`] is masked for the given [`PackageView`].
    pub fn is_masked_for_pkg<P: PackageView>(&self, pkg: &P, flag: &UseFlag) -> bool {
        if self.is_masked(flag) {
            return true;
        }

        match Self::find_pkguse_match(pkg, flag, &self.package_use_mask) {
            Some(mask) => match Self::find_pkguse_match(pkg, flag, &self.package_use_stable_mask) {
                Some(stable_mask) => mask.max(stable_mask).op.as_bool(),
                None => mask.op.as_bool(),
            },
            None => false,
        }
    }

    /// Checks if the given [`UseFlag`] is forced.
    pub fn is_forced(&self, flag: &UseFlag) -> bool {
        self.use_force.contains(flag) || self.use_stable_force.contains(flag)
    }

    /// Checks if the given [`UseFlag`] is forced for the given [`PackageView`].
    pub fn is_forced_for_pkg<P: PackageView>(&self, pkg: &P, flag: &UseFlag) -> bool {
        if self.is_forced(flag) {
            return true;
        }

        match Self::find_pkguse_match(pkg, flag, &self.package_use_force) {
            Some(mask) => {
                match Self::find_pkguse_match(pkg, flag, &self.package_use_stable_force) {
                    Some(stable_mask) => mask.max(stable_mask).op.as_bool(),
                    None => mask.op.as_bool(),
                }
            }
            None => false,
        }
    }

    /// Returns the match with the highest precedence from the given `map`.
    fn find_pkguse_match<'a, P: PackageView>(
        pkg: &P,
        flag: &UseFlag,
        map: &'a FxHashMap<Atom, EntryUseFlags>,
    ) -> Option<&'a Entry<UseFlag>> {
        map.iter()
            .filter_map(|(atom, flags)| {
                pkg.matches_atom(atom)
                    .then(|| flags.get_match(flag))
                    .flatten()
            })
            .max()
    }
}
