use crate::deps::atom::Atom;
use crate::deps::useflag::UseFlag;
use crate::files::UseEntries;
use crate::files::pkguse::PackageUseEntries;
use crate::package::Package;
use crate::profile::Profile;
use crate::types::{FxHashMap, FxHashSet};
use crate::utils::Inherit;

pub struct UseMasks {
    use_mask: FxHashSet<UseFlag>,
    use_force: FxHashSet<UseFlag>,
    use_stable_mask: FxHashSet<UseFlag>,
    use_stable_force: FxHashSet<UseFlag>,

    package_use: FxHashMap<Atom, FxHashSet<UseFlag>>,
    package_use_mask: FxHashMap<Atom, FxHashSet<UseFlag>>,
    package_use_force: FxHashMap<Atom, FxHashSet<UseFlag>>,
    package_use_stable_mask: FxHashMap<Atom, FxHashSet<UseFlag>>,
    package_use_stable_force: FxHashMap<Atom, FxHashSet<UseFlag>>,
}

impl UseMasks {
    pub fn new(
        profile: &Profile,
        package_use: PackageUseEntries,
        use_mask: UseEntries,
        package_use_mask: PackageUseEntries,
    ) -> Self {
        let use_mask = use_mask
            .inherit(&profile.use_mask)
            .into_iter()
            .filter_map(|e| e.into_value())
            .collect::<FxHashSet<_>>();
        let use_force = profile
            .use_force
            .iter()
            .filter_map(|e| e.clone().into_value())
            .collect::<FxHashSet<_>>();
        let use_stable_mask = profile
            .use_stable_mask
            .iter()
            .filter_map(|e| e.clone().into_value())
            .collect::<FxHashSet<_>>();
        let use_stable_force = profile
            .use_stable_force
            .iter()
            .filter_map(|e| e.clone().into_value())
            .collect::<FxHashSet<_>>();

        let package_use = package_use.inherit(&profile.package_use).finalize();

        let package_use_mask = package_use_mask
            .inherit(&profile.package_use_mask)
            .finalize();
        let package_use_force = profile.package_use_force.clone().finalize();
        let package_use_stable_mask = profile.package_use_stable_mask.clone().finalize();
        let package_use_stable_force = profile.package_use_stable_force.clone().finalize();

        Self {
            use_mask,
            use_force,
            use_stable_mask,
            use_stable_force,
            package_use,
            package_use_mask,
            package_use_force,
            package_use_stable_mask,
            package_use_stable_force,
        }
    }

    /// Checks if the given [`UseFlag`] is masked.
    pub fn is_masked(&self, flag: &UseFlag) -> bool {
        self.use_mask.contains(flag) || self.use_stable_mask.contains(flag)
    }

    /// Checks if the given [`UseFlag`] is masked for the given [`Package`].
    pub fn is_masked_for_pkg(&self, pkg: &Package, flag: &UseFlag) -> bool {
        if self.is_masked(flag) {
            return true;
        }

        for (atom, flags) in &self.package_use_mask {
            if pkg.matches_atom(atom) && flags.contains(flag) {
                return true;
            }
        }
        for (atom, flags) in &self.package_use_stable_mask {
            if pkg.matches_atom(atom) && flags.contains(flag) {
                return true;
            }
        }
        false
    }
}
