use crate::deps::atom::Atom;
use crate::files::UseEntries;
use crate::files::entry::Entry;
use crate::files::pkgfile::{PackageUsePolicy, UseFlags};
use crate::makenv::MakeEnv;
use crate::package::PackageView;
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::{UseExpandConfig, UseFlag};
use crate::utils::Inherit;
use anyhow::Context;

/// Simple DTO used to build [`UseMasks`] from profile USE entries.
#[derive(Default)]
pub struct ProfileRecords {
    pub make_defaults: MakeEnv,
    pub package_use: PackageUsePolicy,
    pub package_use_mask: PackageUsePolicy,
    pub package_use_force: PackageUsePolicy,
    pub package_use_stable_mask: PackageUsePolicy,
    pub package_use_stable_force: PackageUsePolicy,
    pub use_mask: UseEntries,
    pub use_force: UseEntries,
    pub use_stable_mask: UseEntries,
    pub use_stable_force: UseEntries,
}

/// Simple DTO used to build [`UseMasks`] from user configured USE entries.
#[derive(Default)]
pub struct LocalRecords {
    pub package_use: PackageUsePolicy,
    pub use_mask: UseEntries,
    pub package_use_mask: PackageUsePolicy,
}

/// Immutable runtime policy that determines whether a USE flag is masked or forced.
pub struct UseMasks {
    use_mask: FxHashSet<UseFlag>,
    use_force: FxHashSet<UseFlag>,
    use_stable_mask: FxHashSet<UseFlag>,
    use_stable_force: FxHashSet<UseFlag>,
    #[allow(unused)]
    package_use: FxHashMap<Atom, UseFlags>,
    package_use_mask: FxHashMap<Atom, UseFlags>,
    package_use_force: FxHashMap<Atom, UseFlags>,
    package_use_stable_mask: FxHashMap<Atom, UseFlags>,
    package_use_stable_force: FxHashMap<Atom, UseFlags>,
}

impl UseMasks {
    /// Builds [`UseMasks`] from profile and local records.
    pub fn new(profile: ProfileRecords, local: LocalRecords) -> anyhow::Result<Self> {
        let expand_conf = UseExpandConfig::from_make_env(&profile.make_defaults)
            .with_context(|| "failed to build the package USE expansion namespace")?;

        Ok(Self {
            use_mask: local
                .use_mask
                .inherit(&profile.use_mask)?
                .into_iter()
                .map(Entry::into_inner)
                .collect(),
            use_force: profile
                .use_force
                .into_iter()
                .map(Entry::into_inner)
                .collect(),
            use_stable_mask: profile
                .use_stable_mask
                .into_iter()
                .map(Entry::into_inner)
                .collect(),
            use_stable_force: profile
                .use_stable_force
                .into_iter()
                .map(Entry::into_inner)
                .collect(),
            package_use: local
                .package_use
                .inherit(&profile.package_use)?
                .expand(&expand_conf)
                .with_context(|| "failed to resolve package.use")?,
            package_use_mask: local
                .package_use_mask
                .inherit(&profile.package_use_mask)?
                .expand(&expand_conf)
                .with_context(|| "failed to resolve package.use.mask")?,
            package_use_force: profile
                .package_use_force
                .expand(&expand_conf)
                .with_context(|| "failed to resolve package.use.force")?,
            package_use_stable_mask: profile
                .package_use_stable_mask
                .expand(&expand_conf)
                .with_context(|| "failed to resolve package.use.stable.mask")?,
            package_use_stable_force: profile
                .package_use_stable_force
                .expand(&expand_conf)
                .with_context(|| "failed to resolve package.use.stable.force")?,
        })
    }

    /// Checks whether `flag` is masked.
    pub fn is_masked(&self, flag: &UseFlag) -> bool {
        self.use_mask.contains(flag) || self.use_stable_mask.contains(flag)
    }

    /// Checks whether `flag` is masked for `pkg`.
    pub fn is_masked_for_pkg<P: PackageView>(&self, pkg: &P, flag: &UseFlag) -> bool {
        if self.is_masked(flag) {
            return true;
        }

        let mask = Self::find_package_use_match(pkg, flag, &self.package_use_mask);
        let stable_mask = Self::find_package_use_match(pkg, flag, &self.package_use_stable_mask);
        match (mask, stable_mask) {
            (Some(mask), Some(stable_mask)) => mask.max(stable_mask).op.as_bool(),
            (Some(mask), None) => mask.op.as_bool(),
            (None, Some(stable_mask)) => stable_mask.op.as_bool(),
            (None, None) => false,
        }
    }

    /// Checks whether `flag` is forced.
    pub fn is_forced(&self, flag: &UseFlag) -> bool {
        self.use_force.contains(flag) || self.use_stable_force.contains(flag)
    }

    /// Checks whether `flag` is forced for `pkg`.
    pub fn is_forced_for_pkg<P: PackageView>(&self, pkg: &P, flag: &UseFlag) -> bool {
        if self.is_forced(flag) {
            return true;
        }

        let force = Self::find_package_use_match(pkg, flag, &self.package_use_force);
        let stable_force = Self::find_package_use_match(pkg, flag, &self.package_use_stable_force);
        match (force, stable_force) {
            (Some(force), Some(stable_force)) => force.max(stable_force).op.as_bool(),
            (Some(force), None) => force.op.as_bool(),
            (None, Some(stable_force)) => stable_force.op.as_bool(),
            (None, None) => false,
        }
    }

    fn find_package_use_match<'a, P: PackageView>(
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
    use crate::package::Package;
    use crate::package::metadata::PackageMetadata;
    use crate::test_support::cpv;

    #[test]
    fn test_package_use_policies() -> anyhow::Result<()> {
        let masks = UseMasks::new(
            ProfileRecords {
                make_defaults: MakeEnv::from_string(
                    "USE_EXPAND=\"LLVM_TARGETS\"
                        USE_EXPAND_UNPREFIXED=\"ARCH\""
                        .into(),
                )?,
                package_use_force: PackageUsePolicy::from_string(
                    "dev-lang/rust rustfmt LLVM_TARGETS: AMDGPU".into(),
                    Precedence::Profile(0),
                )?,
                package_use_stable_mask: PackageUsePolicy::from_string(
                    "dev-lang/rust LLVM_TARGETS: X86".into(),
                    Precedence::Profile(0),
                )?,
                package_use_stable_force: PackageUsePolicy::from_string(
                    "dev-lang/rust ARCH: amd64".into(),
                    Precedence::Profile(0),
                )?,
                ..Default::default()
            },
            LocalRecords {
                package_use_mask: PackageUsePolicy::from_string(
                    "dev-lang/rust wasm".into(),
                    Precedence::User,
                )?,
                ..Default::default()
            },
        )?;

        let cpv = cpv("dev-lang", "rust", "1.97.1");
        let repo = "gentoo".parse().unwrap();
        let package = Package::new(cpv, repo, PackageMetadata::default());
        assert!(masks.is_masked_for_pkg(&package, &UseFlag::new("wasm")?));
        assert!(masks.is_masked_for_pkg(&package, &UseFlag::new("llvm_targets_X86")?));
        assert!(masks.is_forced_for_pkg(&package, &UseFlag::new("rustfmt")?));
        assert!(masks.is_forced_for_pkg(&package, &UseFlag::new("llvm_targets_AMDGPU")?));
        assert!(masks.is_forced_for_pkg(&package, &UseFlag::new("amd64")?));
        Ok(())
    }
}
