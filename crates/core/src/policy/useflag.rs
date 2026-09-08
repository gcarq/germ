use crate::deps::atom::Atom;
use crate::files::pkgfile::{PackageUseRecords, UseFlags};
use crate::files::{UseEntries, entry::Entry};
use crate::package::PackageView;
use crate::profile::ProfileUseRecords;
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::{IUseEntry, IUseState, UseExpandConfig, UseFlag};
use crate::utils::Inherit;
use anyhow::Context;

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
    base_use: UseRules,
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
    /// Builds [`UsePolicy`] from global, profile, and local records.
    pub fn new(
        global_use: FxHashMap<UseFlag, bool>,
        iuse_implicit: FxHashSet<UseFlag>,
        profile: ProfileUseRecords,
        local: LocalRecords,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            base_use: UseRules(global_use),
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
                &profile.expand_config,
            )
            .context("failed to resolve package.use")?,
            package_use_mask: PackageUse::new(
                local.package_use_mask.inherit(&profile.package_use_mask)?,
                &profile.expand_config,
            )
            .context("failed to resolve package.use.mask")?,
            package_use_force: PackageUse::new(profile.package_use_force, &profile.expand_config)
                .context("failed to resolve package.use.force")?,
            package_use_stable_mask: PackageUse::new(
                profile.package_use_stable_mask,
                &profile.expand_config,
            )
            .context("failed to resolve package.use.stable.mask")?,
            package_use_stable_force: PackageUse::new(
                profile.package_use_stable_force,
                &profile.expand_config,
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

        let package_use = self.package_use.entries_for(pkg);
        let masked = self.masked_for_pkg(pkg, stable_in_use);
        let forced = self.forced_for_pkg(pkg, stable_in_use);

        let enabled = available
            .iter()
            .filter(|flag| !masked.contains(*flag))
            .filter(|flag| {
                forced.contains(*flag)
                    || self.desired_state(pkg, &package_use, flag).unwrap_or(false)
            })
            .copied()
            .collect();

        EffectiveUse { available, enabled }
    }

    /// Returns the desired state of `flag` for `pkg`.
    fn desired_state<P: PackageView>(
        &self,
        pkg: &P,
        package_use: &FxHashMap<&UseFlag, &Entry<UseFlag>>,
        flag: &UseFlag,
    ) -> Option<bool> {
        package_use
            .get(flag)
            .map(|entry| entry.op.as_bool())
            .or_else(|| self.base_use.state(flag))
            .or_else(|| {
                pkg.metadata()
                    .iuse
                    .iter()
                    .find(|entry| entry.flag() == flag)
                    .and_then(|entry| entry.state().map(IUseState::as_bool))
            })
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

/// Runtime USE policy assignments.
///
/// It is a simple mapping of [`UseFlag`] to `bool` indicating
/// whether the flag is enabled or disabled.
#[derive(Clone, Default)]
struct UseRules(FxHashMap<UseFlag, bool>);

impl UseRules {
    /// Returns the assignment for the given `flag`.
    fn state(&self, flag: &UseFlag) -> Option<bool> {
        self.0.get(flag).copied()
    }
}

/// Runtime policy to determine the effective USE flags for a package.
struct PackageUse(Vec<(Atom, UseFlags)>);

impl PackageUse {
    fn new(records: PackageUseRecords, config: &UseExpandConfig) -> anyhow::Result<Self> {
        Ok(Self(records.expand(config)?))
    }

    /// Returns package USE entries that apply to the given `pkg`.
    fn entries_for<'a, P: PackageView>(
        &'a self,
        pkg: &P,
    ) -> FxHashMap<&'a UseFlag, &'a Entry<UseFlag>> {
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
    }

    /// Returns all enabled USE flags that apply to the given `pkg`.
    fn enabled_for<'a, P: PackageView>(&'a self, pkg: &P) -> impl Iterator<Item = &'a UseFlag> {
        self.entries_for(pkg)
            .into_values()
            .filter(|entry| entry.op.as_bool())
            .map(Entry::inner)
    }
}

/// Final USE state of one package.
#[cfg_attr(not(test), expect(dead_code))]
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
    #[cfg_attr(not(test), expect(dead_code))]
    fn state(&self, flag: &UseFlag) -> Option<bool> {
        self.available
            .contains(flag)
            .then(|| self.enabled.contains(flag))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::entry::Precedence;
    use crate::makenv::{MakeEnv, MakeEnvStack};

    use crate::package::Package;
    use crate::package::metadata::PackageMetadata;
    use crate::test_support::cpv;

    fn use_state(
        makenv: &str,
        make_conf: &str,
        package_use: &str,
        iuse: &str,
        flag: &str,
    ) -> anyhow::Result<Option<bool>> {
        let global = MakeEnv::from_string(makenv.into())?;
        let user = MakeEnv::from_string(make_conf.into())?;
        let makenv_stack = MakeEnvStack::new(global, MakeEnv::default(), user)?;
        let local = LocalRecords {
            package_use: PackageUseRecords::from_string(package_use.into(), Precedence::User)?,
            ..Default::default()
        };
        let policy = UsePolicy::new(
            makenv_stack.global_use()?,
            FxHashSet::default(),
            ProfileUseRecords::default(),
            local,
        )?;
        let package = Package::new(
            cpv("dev-lang", "rust", "1.0"),
            "gentoo".parse()?,
            PackageMetadata {
                iuse: iuse
                    .split_whitespace()
                    .map(str::parse)
                    .collect::<anyhow::Result<_>>()?,
                ..Default::default()
            },
        );
        Ok(policy
            .effective_for(&package, false)
            .state(&UseFlag::new(flag)?))
    }

    #[test]
    fn test_effective_use() {
        // makenv, make_conf, package_use, iuse, flag, expected
        let cases = [
            ("USE=foo", "", "*/* -foo", "+foo", "foo", Some(false)),
            ("", "USE=-foo", "*/* foo", "foo", "foo", Some(true)),
            ("USE=foo", "", "", "", "foo", None),
            ("", "", "", "+foo", "foo", Some(true)),
            ("", "USE=-foo", "", "+foo", "foo", Some(false)),
        ];

        for case in cases {
            assert_eq!(
                use_state(case.0, case.1, case.2, case.3, case.4).unwrap(),
                case.5
            );
        }
    }
}
