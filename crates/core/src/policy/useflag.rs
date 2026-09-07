use crate::deps::atom::Atom;
use crate::files::UseEntries;
use crate::files::entry::Entry;
use crate::files::pkgfile::{PackageUseRecords, UseFlags};
use crate::makenv::MakeEnv;
use crate::package::PackageView;
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::{IUseEntry, UseExpandConfig, UseFlag};
use crate::utils::Inherit;
use anyhow::Context;

/// Simple DTO used to build [`UsePolicy`] from profile USE entries.
#[derive(Default)]
pub struct ProfileRecords {
    pub make_defaults: MakeEnv,
    pub package_use: PackageUseRecords,
    pub package_use_mask: PackageUseRecords,
    pub package_use_force: PackageUseRecords,
    pub package_use_stable_mask: PackageUseRecords,
    pub package_use_stable_force: PackageUseRecords,
    pub use_mask: UseEntries,
    pub use_force: UseEntries,
    pub use_stable_mask: UseEntries,
    pub use_stable_force: UseEntries,
}

/// Simple DTO used to build [`UsePolicy`] from user configured USE entries.
#[derive(Default)]
pub struct LocalRecords {
    pub package_use: PackageUseRecords,
    pub use_mask: UseEntries,
    pub package_use_mask: PackageUseRecords,
}

/// Immutable runtime policy used to calculate effective
/// USE flags and evaluate `REQUIRED_USE`.
#[expect(dead_code)]
pub struct UsePolicy {
    iuse_implicit: FxHashSet<UseFlag>,

    use_mask: FxHashSet<UseFlag>,
    use_force: FxHashSet<UseFlag>,

    use_stable_mask: FxHashSet<UseFlag>,
    use_stable_force: FxHashSet<UseFlag>,

    package_use: PackageUse,
    package_use_mask: PackageUse,
    package_use_force: PackageUse,

    package_use_stable_mask: PackageUse,
    package_use_stable_force: PackageUse,
}

impl UsePolicy {
    /// Builds [`UsePolicy`] from profile and local records.
    pub fn new(
        iuse_implicit: FxHashSet<UseFlag>,
        profile: ProfileRecords,
        local: LocalRecords,
    ) -> anyhow::Result<Self> {
        let expand_conf = UseExpandConfig::from_make_env(&profile.make_defaults)
            .context("failed to build the USE expand config")?;

        Ok(Self {
            use_mask: local
                .use_mask
                .inherit(&profile.use_mask)?
                .into_inner()
                .collect(),
            use_force: profile.use_force.into_inner().collect(),
            use_stable_mask: profile.use_stable_mask.into_inner().collect(),
            use_stable_force: profile.use_stable_force.into_inner().collect(),
            package_use: PackageUse::new(
                local.package_use.inherit(&profile.package_use)?,
                &expand_conf,
            )
            .context("failed to resolve package.use")?,
            package_use_mask: PackageUse::new(
                local.package_use_mask.inherit(&profile.package_use_mask)?,
                &expand_conf,
            )
            .context("failed to resolve package.use.mask")?,
            package_use_force: PackageUse::new(profile.package_use_force, &expand_conf)
                .context("failed to resolve package.use.force")?,
            package_use_stable_mask: PackageUse::new(profile.package_use_stable_mask, &expand_conf)
                .context("failed to resolve package.use.stable.mask")?,
            package_use_stable_force: PackageUse::new(
                profile.package_use_stable_force,
                &expand_conf,
            )
            .context("failed to resolve package.use.stable.force")?,
            iuse_implicit,
        })
    }

    /// Returns `true` if all required USE flags are satisfied for the given [`PackageView`].
    pub fn required_use_satisfied<P: PackageView>(
        &self,
        pkg: &P,
        stable_in_use: bool,
    ) -> anyhow::Result<bool> {
        let _use_state = self.effective_for(pkg, stable_in_use);
        // TODO: implement me
        Ok(true)
    }

    /// Returns the effective USE state for the given [`PackageView`].
    fn effective_for<'a, P: PackageView>(
        &'a self,
        pkg: &'a P,
        stable_in_use: bool,
    ) -> EffectiveUse<'a> {
        let available = pkg
            .metadata()
            .iuse
            .iter()
            .map(IUseEntry::flag)
            .chain(self.iuse_implicit.iter())
            .collect::<FxHashSet<_>>();

        let requested = self.package_use.enabled_for(pkg).collect::<FxHashSet<_>>();
        let masked = self.masked_for_pkg(pkg, stable_in_use);
        let forced = self.forced_for_pkg(pkg, stable_in_use);

        let enabled = available
            .iter()
            .filter(|flag| !masked.contains(*flag))
            .filter(|flag| forced.contains(*flag) || requested.contains(*flag))
            .copied()
            .collect();

        EffectiveUse { available, enabled }
    }

    /// Returns all USE flags that are masked for the given [`PackageView`].
    fn masked_for_pkg<P: PackageView>(&self, pkg: &P, stable_in_use: bool) -> FxHashSet<&UseFlag> {
        let iter = self.package_use_mask.enabled_for(pkg);
        match stable_in_use {
            true => iter
                .chain(self.package_use_stable_mask.enabled_for(pkg))
                .collect(),
            false => iter.collect(),
        }
    }

    /// Returns all USE flags that are forced for the given [`PackageView`].
    fn forced_for_pkg<P: PackageView>(&self, pkg: &P, stable_in_use: bool) -> FxHashSet<&UseFlag> {
        let iter = self.package_use_force.enabled_for(pkg);
        match stable_in_use {
            true => iter
                .chain(self.package_use_stable_force.enabled_for(pkg))
                .collect(),
            false => iter.collect(),
        }
    }
}

/// Runtime policy to determine the effective USE flags for a package.
struct PackageUse(Vec<(Atom, UseFlags)>);

impl PackageUse {
    fn new(records: PackageUseRecords, config: &UseExpandConfig) -> anyhow::Result<Self> {
        Ok(Self(records.resolve(config)?))
    }

    /// Returns an iterator over all USE flags that apply to the given `pkg`.
    ///
    /// The yielded tuple contains the [`UseFlag`] and the enabled state.
    fn flags_for<'a, P: PackageView>(
        &'a self,
        pkg: &P,
    ) -> impl Iterator<Item = (&'a UseFlag, bool)> {
        let mut flags: FxHashMap<&UseFlag, &Entry<UseFlag>> = FxHashMap::default();

        for (atom, cur_flags) in &self.0 {
            if !pkg.matches_atom(atom) {
                continue;
            }

            for entry in cur_flags.iter() {
                match flags.get(entry.inner()) {
                    Some(existing) if existing.prec > entry.prec => continue,
                    _ => flags.insert(entry.inner(), entry),
                };
            }
        }
        flags
            .into_iter()
            .map(|(flag, entry)| (flag, entry.op.as_bool()))
    }

    /// Returns an iterator over all enabled USE flags that apply to the given `pkg`.
    fn enabled_for<'a, P: PackageView>(&'a self, pkg: &P) -> impl Iterator<Item = &'a UseFlag> {
        self.flags_for(pkg)
            .filter(|(_, enabled)| *enabled)
            .map(|(flag, _)| flag)
    }
}

/// Final USE state of one package.
#[expect(dead_code)]
struct EffectiveUse<'a> {
    /// All flags that can exist for the package.
    /// This corresponds to `IUSE_EFFECTIVE`.
    available: FxHashSet<&'a UseFlag>,
    /// Available flags that are enabled.
    enabled: FxHashSet<&'a UseFlag>,
}

impl<'a> EffectiveUse<'a> {
    /// Returns the effective state of the given `flag`.
    /// - `Some(true)` if the flag is enabled.
    /// - `Some(false)` if the flag is disabled.
    /// - `None` if the flag is not available.
    #[expect(dead_code)]
    pub fn state(&self, flag: &UseFlag) -> Option<bool> {
        self.available
            .contains(flag)
            .then(|| self.enabled.contains(flag))
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
    fn test_package_use_flags_for() -> anyhow::Result<()> {
        let package_use = PackageUse::new(
            PackageUseRecords::from_string(
                "*/* foo -bar
                    dev-lang/rust -foo baz"
                    .into(),
                Precedence::User,
            )?,
            &UseExpandConfig::default(),
        )?;
        let package = Package::new(
            cpv("dev-lang", "rust", "1.0"),
            "gentoo".parse()?,
            PackageMetadata::default(),
        );
        let flags = package_use.flags_for(&package).collect::<FxHashMap<_, _>>();

        assert_eq!(flags.get(&UseFlag::new("foo")?), Some(&false));
        assert_eq!(flags.get(&UseFlag::new("bar")?), Some(&false));
        assert_eq!(flags.get(&UseFlag::new("baz")?), Some(&true));
        assert_eq!(flags.get(&UseFlag::new("qux")?), None);
        Ok(())
    }
}
