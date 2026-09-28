use crate::atom::Atom;

use crate::deps::{ExprEval, RequiredUseFlag};
use crate::files::pkgfile::{PackageUseRecords, UseFlags};
use crate::files::{UseEntries, entry::Entry};
use crate::package::PackageView;
use crate::profile::ProfileUseRecords;
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::{EffectiveUse, IUseEntry, IUseState, UseExpandConfig, UseFlag};
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
                .finalize()
                .collect(),
            use_force: profile.use_force.finalize().collect(),
            use_stable_mask: profile.use_stable_mask.finalize().collect(),
            use_stable_force: profile.use_stable_force.finalize().collect(),
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

    /// Evaluates the required USE flags and returns a tuple containing its [`EffectiveUse`]
    /// and whether the required USE flags are satisfied.
    ///
    /// TODO: This currently only checks the required USE flags to satisfy the expression,
    ///       inactive branches can contain unreferenced USE flags which might not be PMS compatible.
    pub fn eval<P>(&self, pkg: &P, stable_in_use: bool) -> anyhow::Result<(EffectiveUse, bool)>
    where
        P: PackageView,
    {
        let effective_use = self.effective_for(pkg, stable_in_use);
        let mut evaluator = ReqUseEval {
            state: &effective_use,
        };
        let use_satisfied = pkg.metadata().required_use().view().eval(&mut evaluator)?;
        Ok((effective_use, use_satisfied))
    }

    /// Returns the effective USE state for the given [`PackageView`].
    fn effective_for<P>(&self, pkg: &P, stable_in_use: bool) -> EffectiveUse
    where
        P: PackageView,
    {
        let available = pkg
            .metadata()
            .iuse()
            .iter()
            .map(IUseEntry::flag)
            .chain(self.iuse_implicit.iter())
            .cloned()
            .collect::<FxHashSet<_>>();

        let pkg_use = self.package_use.entries_for(pkg);
        let masked = self.masked_for_pkg(pkg, stable_in_use);
        let forced = self.forced_for_pkg(pkg, stable_in_use);

        let enabled = available
            .iter()
            .filter(|flag| !masked.contains(*flag))
            .filter(|flag| {
                forced.contains(*flag) || self.desired_state(pkg, &pkg_use, flag).unwrap_or(false)
            })
            .cloned()
            .collect();

        EffectiveUse::from_parts(available, enabled)
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
                    .iuse()
                    .iter()
                    .find(|entry| entry.flag() == flag)
                    .and_then(|entry| entry.state().map(IUseState::as_bool))
            })
    }

    /// Returns all USE flags that are masked for the given [`PackageView`].
    fn masked_for_pkg<P: PackageView>(&self, pkg: &P, stable_in_use: bool) -> FxHashSet<&UseFlag> {
        let iter = self
            .use_mask
            .iter()
            .chain(self.package_use_mask.enabled_for(pkg));
        match stable_in_use {
            true => iter
                .chain(self.use_stable_mask.iter())
                .chain(self.package_use_stable_mask.enabled_for(pkg))
                .collect(),
            false => iter.collect(),
        }
    }

    /// Returns all USE flags that are forced for the given [`PackageView`].
    fn forced_for_pkg<P: PackageView>(&self, pkg: &P, stable_in_use: bool) -> FxHashSet<&UseFlag> {
        let iter = self
            .use_force
            .iter()
            .chain(self.package_use_force.enabled_for(pkg));
        match stable_in_use {
            true => iter
                .chain(self.use_stable_force.iter())
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
    ///
    /// The USE policy is matched statically, atom USE deps are not considered.
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

/// Evaluates `REQUIRED_USE` against the effective USE state of a given package.
struct ReqUseEval<'a> {
    state: &'a EffectiveUse,
}

impl ExprEval<RequiredUseFlag> for ReqUseEval<'_> {
    fn eval_item(&mut self, flag: &RequiredUseFlag) -> anyhow::Result<bool> {
        match flag.is_negated() {
            true => self.state.is_disabled(flag.inner()),
            false => self.state.is_enabled(flag.inner()),
        }
    }

    fn is_use_enabled(&self, flag: &UseFlag) -> anyhow::Result<bool> {
        self.state.is_enabled(flag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deps::{DepExpr, ExprKind};
    use crate::eapi::Eapi;
    use crate::files::entry::Precedence;
    use crate::makenv::{MakeEnv, MakeEnvStack};
    use crate::package::Package;
    use crate::test_support::{cpv, pkg_metadata};

    fn use_state(
        makenv: &str,
        make_conf: &str,
        package_use: &str,
        iuse: &str,
        flag: &str,
    ) -> anyhow::Result<Option<bool>> {
        let global = MakeEnv::from_content(makenv)?;
        let user = MakeEnv::from_content(make_conf)?;
        let makenv_stack = MakeEnvStack::new(global, MakeEnv::default(), user)?;
        let local = LocalRecords {
            package_use: PackageUseRecords::from_content(package_use, Precedence::User)?,
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
            pkg_metadata(&[("IUSE", iuse)]),
        );
        Ok(policy
            .effective_for(&package, false)
            .state(&UseFlag::new(flag)?))
    }

    #[test]
    fn test_eval_required_use() -> anyhow::Result<()> {
        // expression, foo state, bar state, expected
        let cases = [
            ("foo", true, false, true),
            ("foo", false, false, false),
            ("!foo", false, false, true),
            ("!foo", true, false, false),
            ("foo? ( bar )", false, true, true),
            ("foo? ( bar )", true, false, false),
        ];

        let foo = UseFlag::new("foo")?;
        let bar = UseFlag::new("bar")?;
        let available = FxHashSet::from_iter([foo.clone(), bar.clone()]);
        for (input, foo_state, bar_state, expected) in cases {
            let expr =
                DepExpr::<RequiredUseFlag>::parse(Eapi::Eight, ExprKind::RequiredUse, input)?;
            let state = EffectiveUse::from_parts(
                available.clone(),
                [(&foo, foo_state), (&bar, bar_state)]
                    .into_iter()
                    .filter_map(|(flag, enabled)| enabled.then_some(flag.clone()))
                    .collect(),
            );
            let mut evaluator = ReqUseEval { state: &state };
            let actual = expr.view().eval(&mut evaluator)?;

            assert_eq!(actual, expected, "{input}");
        }
        Ok(())
    }

    #[test]
    fn test_profile_use_constraints() -> anyhow::Result<()> {
        let elogind = UseFlag::new("elogind")?;
        let systemd = UseFlag::new("systemd")?;
        let forced = UseFlag::new("forced")?;
        let profile = ProfileUseRecords {
            use_mask: UseEntries::from_content("elogind", Precedence::Profile(0))?,
            use_force: UseEntries::from_content("forced", Precedence::Profile(0))?,
            ..Default::default()
        };
        let policy = UsePolicy::new(
            FxHashMap::from_iter([(elogind.clone(), true), (systemd.clone(), true)]),
            FxHashSet::default(),
            profile,
            LocalRecords::default(),
        )?;
        let package = Package::new(
            cpv("sys-libs", "pam", "1.0"),
            "gentoo".parse()?,
            pkg_metadata(&[("IUSE", "elogind systemd forced")]),
        );
        let effective = policy.effective_for(&package, false);

        assert_eq!(effective.state(&elogind), Some(false));
        assert_eq!(effective.state(&systemd), Some(true));
        assert_eq!(effective.state(&forced), Some(true));
        Ok(())
    }

    #[test]
    fn test_profile_use_unmask() -> anyhow::Result<()> {
        let elogind = UseFlag::new("elogind")?;
        let masked = UseEntries::from_content("elogind", Precedence::Profile(0))?;
        let profile = ProfileUseRecords {
            use_mask: UseEntries::from_content("-elogind", Precedence::Profile(1))?
                .inherit(&masked)?,
            ..Default::default()
        };
        let policy = UsePolicy::new(
            FxHashMap::from_iter([(elogind.clone(), true)]),
            FxHashSet::default(),
            profile,
            LocalRecords::default(),
        )?;
        let package = Package::new(
            cpv("sys-libs", "pam", "1.0"),
            "gentoo".parse()?,
            pkg_metadata(&[("IUSE", "elogind")]),
        );

        assert_eq!(
            policy.effective_for(&package, false).state(&elogind),
            Some(true)
        );
        Ok(())
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
