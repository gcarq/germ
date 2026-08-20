use crate::deps::atom::Atom;
use crate::files::UseEntries;
use crate::files::entry::Entry;
use crate::files::pkguse::{PackageUseEntries, UseFlags};
use crate::package::PackageView;
use crate::profile::Profile;
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::{UseExpandConfig, UseFlag};
use crate::utils::Inherit;
use anyhow::Context;

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
    package_use: FxHashMap<Atom, UseFlags>,
    // Masked USE flags on a per-package basis
    package_use_mask: FxHashMap<Atom, UseFlags>,
    // Forced USE flags on a per-package basis
    package_use_force: FxHashMap<Atom, UseFlags>,
    // Same as above but for merged packages due to a stable keyword
    package_use_stable_mask: FxHashMap<Atom, UseFlags>,
    package_use_stable_force: FxHashMap<Atom, UseFlags>,
}

impl UseMasks {
    pub fn new(
        profile: &Profile,
        package_use: PackageUseEntries,
        use_mask: UseEntries,
        package_use_mask: PackageUseEntries,
    ) -> anyhow::Result<Self> {
        let expand_conf = UseExpandConfig::from_make_env(&profile.make_defaults)
            .with_context(|| "failed to build the package USE expansion namespace")?;

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

        let package_use = package_use
            .inherit(&profile.package_use)?
            .expand(&expand_conf)
            .with_context(|| "failed to resolve package.use")?;
        let package_use_mask = package_use_mask
            .inherit(&profile.package_use_mask)?
            .expand(&expand_conf)
            .with_context(|| "failed to resolve package.use.mask")?;
        let package_use_force = profile
            .package_use_force
            .clone()
            .expand(&expand_conf)
            .with_context(|| "failed to resolve package.use.force")?;
        let package_use_stable_mask = profile
            .package_use_stable_mask
            .clone()
            .expand(&expand_conf)
            .with_context(|| "failed to resolve package.use.stable.mask")?;
        let package_use_stable_force = profile
            .package_use_stable_force
            .clone()
            .expand(&expand_conf)
            .with_context(|| "failed to resolve package.use.stable.force")?;

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

        let mask = Self::find_pkguse_match(pkg, flag, &self.package_use_mask);
        let stable_mask = Self::find_pkguse_match(pkg, flag, &self.package_use_stable_mask);
        match (mask, stable_mask) {
            (Some(mask), Some(stable_mask)) => mask.max(stable_mask).op.as_bool(),
            (Some(mask), None) => mask.op.as_bool(),
            (None, Some(stable_mask)) => stable_mask.op.as_bool(),
            (None, None) => false,
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

        let force = Self::find_pkguse_match(pkg, flag, &self.package_use_force);
        let stable_force = Self::find_pkguse_match(pkg, flag, &self.package_use_stable_force);
        match (force, stable_force) {
            (Some(force), Some(stable_force)) => force.max(stable_force).op.as_bool(),
            (Some(force), None) => force.op.as_bool(),
            (None, Some(stable_force)) => stable_force.op.as_bool(),
            (None, None) => false,
        }
    }

    /// Returns the match with the highest precedence from the given `map`.
    fn find_pkguse_match<'a, P: PackageView>(
        pkg: &P,
        flag: &UseFlag,
        map: &'a FxHashMap<Atom, UseFlags>,
    ) -> Option<&'a Entry<UseFlag>> {
        map.iter()
            .filter_map(|(atom, flags)| pkg.matches_atom(atom).then(|| flags.get(flag)).flatten())
            .max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::entry::Precedence;
    use crate::makenv::MakeEnv;
    use crate::package::Package;
    use crate::package::metadata::PackageMetadata;
    use crate::test_support::cpv;

    fn profile_with_expansions() -> anyhow::Result<Profile> {
        let mut profile = Profile::default();
        profile.make_defaults = MakeEnv::from_string(
            "USE_EXPAND=\"LLVM_TARGETS\"\nUSE_EXPAND_UNPREFIXED=\"ARCH\"".into(),
        )?;
        Ok(profile)
    }

    #[test]
    fn test_package_use_mask() -> anyhow::Result<()> {
        let profile = profile_with_expansions()?;
        let package_use_mask = PackageUseEntries::from_string(
            "dev-lang/rust wasm LLVM_TARGETS: AMDGPU ARCH: amd64".into(),
            Precedence::User,
        )?;
        let masks = UseMasks::new(
            &profile,
            PackageUseEntries::default(),
            UseEntries::default(),
            package_use_mask,
        )?;

        let cpv = cpv("dev-lang", "rust", "1.97.1");
        let repo = "gentoo".parse().unwrap();
        let package = Package::new(&cpv, &repo, PackageMetadata::default());
        assert!(masks.is_masked_for_pkg(&package, &UseFlag::new("wasm")?));
        assert!(masks.is_masked_for_pkg(&package, &UseFlag::new("llvm_targets_AMDGPU")?));
        assert!(masks.is_masked_for_pkg(&package, &UseFlag::new("amd64")?));
        Ok(())
    }

    #[test]
    fn test_package_use_force() -> anyhow::Result<()> {
        let mut profile = profile_with_expansions()?;
        profile.package_use_force = PackageUseEntries::from_string(
            "dev-lang/rust wasm LLVM_TARGETS: AMDGPU ARCH: amd64".into(),
            Precedence::Profile(0),
        )?;
        let masks = UseMasks::new(
            &profile,
            PackageUseEntries::default(),
            UseEntries::default(),
            PackageUseEntries::default(),
        )?;

        let cpv = cpv("dev-lang", "rust", "1.97.1");
        let repo = "gentoo".parse().unwrap();
        let package = Package::new(&cpv, &repo, PackageMetadata::default());
        assert!(masks.is_forced_for_pkg(&package, &UseFlag::new("wasm")?));
        assert!(masks.is_forced_for_pkg(&package, &UseFlag::new("llvm_targets_AMDGPU")?));
        assert!(masks.is_forced_for_pkg(&package, &UseFlag::new("amd64")?));
        Ok(())
    }

    #[test]
    fn test_stable_package_use_policy() -> anyhow::Result<()> {
        let mut profile = profile_with_expansions()?;
        profile.package_use_stable_mask = PackageUseEntries::from_string(
            "dev-lang/rust LLVM_TARGETS: AMDGPU".into(),
            Precedence::Profile(0),
        )?;
        profile.package_use_stable_force = PackageUseEntries::from_string(
            "dev-lang/rust ARCH: amd64".into(),
            Precedence::Profile(0),
        )?;
        let masks = UseMasks::new(
            &profile,
            PackageUseEntries::default(),
            UseEntries::default(),
            PackageUseEntries::default(),
        )?;

        let cpv = cpv("dev-lang", "rust", "1.97.1");
        let repo = "gentoo".parse().unwrap();
        let package = Package::new(&cpv, &repo, PackageMetadata::default());
        assert!(masks.is_masked_for_pkg(&package, &UseFlag::new("llvm_targets_AMDGPU")?));
        assert!(masks.is_forced_for_pkg(&package, &UseFlag::new("amd64")?));
        Ok(())
    }
}
