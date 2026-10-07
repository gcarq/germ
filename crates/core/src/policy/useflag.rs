use anyhow::Context;
use germ_pms::{ExprEval, IUseState, PackageView, RequiredUseFlag, UseFlag};

use super::index::AtomIndex;
use crate::files::UseEntries;
use crate::files::pkgfile::{MatchedUse, PackageUseRecords, UseFlags};
use crate::profile::ProfileUseRecords;
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::{EffectiveUse, UseExpandConfig};
use crate::utils::Inherit;

/// Simple DTO used to build [`UsePolicy`] from user configured USE entries.
#[derive(Default)]
pub struct LocalRecords {
    pub package_use: PackageUseRecords,
    pub package_use_force: PackageUseRecords,
    pub use_mask: UseEntries,
    pub package_use_mask: PackageUseRecords,
}

/// Holds global USE flags and their enabled state.
///
/// It is a simple mapping of [`UseFlag`] to `bool` indicating
/// whether the flag is enabled or disabled.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct GlobalUseRules(FxHashMap<UseFlag, bool>);

impl GlobalUseRules {
    /// Returns the enabled state for the given `flag`.
    fn state(&self, flag: &UseFlag) -> Option<bool> {
        self.0.get(flag).copied()
    }
}

impl From<FxHashMap<UseFlag, bool>> for GlobalUseRules {
    fn from(map: FxHashMap<UseFlag, bool>) -> Self {
        Self(map)
    }
}

/// Immutable runtime policy used to calculate effective
/// USE flags and evaluate `REQUIRED_USE`.
pub struct UsePolicy {
    global_use: GlobalUseRules,
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
    expand_config: UseExpandConfig,
}

impl UsePolicy {
    /// Builds [`UsePolicy`] from global, profile, and local records.
    pub fn new(
        global_use: GlobalUseRules,
        iuse_implicit: FxHashSet<UseFlag>,
        profile: ProfileUseRecords,
        local: LocalRecords,
    ) -> anyhow::Result<Self> {
        Ok(Self {
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
            package_use_force: PackageUse::new(
                local
                    .package_use_force
                    .inherit(&profile.package_use_force)?,
                &profile.expand_config,
            )
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
            global_use,
            iuse_implicit,
            expand_config: profile.expand_config,
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
        let mut evaluator = ReqUseEval::new(&effective_use);
        let use_satisfied = pkg.metadata().required_use().view().eval(&mut evaluator)?;
        Ok((effective_use, use_satisfied))
    }

    /// Returns the effective USE state for the given [`PackageView`].
    fn effective_for<P>(&self, pkg: &P, stable_in_use: bool) -> EffectiveUse
    where
        P: PackageView,
    {
        let mut available = FxHashSet::default();
        let mut enabled = FxHashSet::default();

        let pkg_use = self.package_use.matched_for(pkg);
        let masked = self.masked_for_pkg(pkg, stable_in_use);
        let forced = self.forced_for_pkg(pkg, stable_in_use);

        let iuse = pkg
            .metadata()
            .iuse()
            .iter()
            .map(|e| (e.flag(), e.state()))
            .chain(self.iuse_implicit.iter().map(|f| (f, None)));

        for (flag, default_state) in iuse {
            if !available.insert(flag.clone()) || masked.contains(flag) {
                continue;
            }

            if forced.contains(flag)
                || pkg_use
                    .state(flag, &self.expand_config)
                    .or_else(|| self.global_use.state(flag))
                    .or_else(|| default_state.map(IUseState::as_bool))
                    .unwrap_or(false)
            {
                enabled.insert(flag.clone());
            }
        }

        EffectiveUse::from_parts(available, enabled)
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

/// Runtime policy to determine the effective USE flags for a package.
struct PackageUse(AtomIndex<UseFlags>);

impl PackageUse {
    fn new(records: PackageUseRecords, config: &UseExpandConfig) -> anyhow::Result<Self> {
        Ok(Self(AtomIndex::new(records.expand(config)?)))
    }

    /// Returns USE entries that apply to the given `pkg`.
    ///
    /// The USE policy is matched statically, atom USE deps are not considered.
    fn matched_for<'a, P: PackageView>(&'a self, pkg: &P) -> MatchedUse<'a> {
        let mut matched = MatchedUse::default();
        self.0.visit_matches(pkg, |flags| matched.absorb(flags));
        matched
    }

    /// Returns all enabled USE flags that apply to the given `pkg`.
    fn enabled_for<'a, P: PackageView>(&'a self, pkg: &P) -> impl Iterator<Item = &'a UseFlag> {
        self.matched_for(pkg).into_enabled()
    }
}

/// Evaluates `REQUIRED_USE` against the effective USE state of a given package.
struct ReqUseEval<'a> {
    state: &'a EffectiveUse,
}

impl ReqUseEval<'_> {
    const fn new(state: &EffectiveUse) -> ReqUseEval<'_> {
        ReqUseEval { state }
    }
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
    use germ_pms::test_support::{cpv, pkg_metadata};
    use germ_pms::{DepExpr, ExprKind, Package};

    use super::*;
    use crate::files::entry::Precedence;
    use crate::makenv::{MakeEnv, MakeEnvStack};

    fn use_state(
        makenv: &str,
        make_conf: &str,
        profile_use: &str,
        package_use: &str,
        iuse: &str,
        implicit: &str,
        flag: &str,
    ) -> anyhow::Result<Option<bool>> {
        let global = MakeEnv::from_content(makenv)?;
        let user = MakeEnv::from_content(make_conf)?;
        let makenv_stack = MakeEnvStack::new(global, MakeEnv::default(), user)?;
        let local = LocalRecords {
            package_use: PackageUseRecords::from_content(package_use, Precedence::User)?,
            ..Default::default()
        };
        let profile = ProfileUseRecords {
            package_use: PackageUseRecords::from_content(profile_use, Precedence::Profile(0))?,
            expand_config: UseExpandConfig::from_makenv(makenv_stack.makenv())?,
            ..Default::default()
        };
        let implicit = implicit
            .split_whitespace()
            .map(UseFlag::new)
            .collect::<anyhow::Result<_>>()?;
        let policy = UsePolicy::new(makenv_stack.global_use()?, implicit, profile, local)?;
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
            let expr = DepExpr::<RequiredUseFlag>::parse(ExprKind::RequiredUse, input)?;
            let state = EffectiveUse::from_parts(
                available.clone(),
                [(&foo, foo_state), (&bar, bar_state)]
                    .into_iter()
                    .filter_map(|(flag, enabled)| enabled.then_some(flag.clone()))
                    .collect(),
            );
            let mut evaluator = ReqUseEval::new(&state);
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
            FxHashMap::from_iter([(elogind.clone(), true), (systemd.clone(), true)]).into(),
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
    fn test_local_package_use_force() -> anyhow::Result<()> {
        let flag = UseFlag::new("llvm_targets_AArch64")?;
        let use_rules = GlobalUseRules::from(FxHashMap::from_iter([(flag.clone(), false)]));
        let expand_config =
            UseExpandConfig::from_makenv(&MakeEnv::from_content("USE_EXPAND=LLVM_TARGETS")?)?;
        let profile = ProfileUseRecords {
            package_use_force: PackageUseRecords::from_content(
                "*/* LLVM_TARGETS: AArch64",
                Precedence::Profile(0),
            )?,
            expand_config,
            ..Default::default()
        };
        let package = Package::new(
            cpv("llvm-core", "clang", "23.1.2"),
            "gentoo".parse()?,
            pkg_metadata(&[("IUSE", "llvm_targets_AArch64")]),
        );
        let forced = UsePolicy::new(
            use_rules.clone(),
            FxHashSet::default(),
            profile.clone(),
            LocalRecords::default(),
        )?;
        assert_eq!(
            forced.effective_for(&package, false).state(&flag),
            Some(true)
        );

        let local = LocalRecords {
            package_use_force: PackageUseRecords::from_content(
                "*/* LLVM_TARGETS: -AArch64",
                Precedence::User,
            )?,
            ..Default::default()
        };
        let unforced = UsePolicy::new(use_rules, FxHashSet::default(), profile, local)?;
        assert_eq!(
            unforced.effective_for(&package, false).state(&flag),
            Some(false)
        );
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
            FxHashMap::from_iter([(elogind.clone(), true)]).into(),
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
    fn test_package_use_group_reset() -> anyhow::Result<()> {
        let makenv = "USE=keep
            USE_EXPAND=LLVM_TARGETS
            LLVM_TARGETS=\"AArch64 ARM AMDGPU BPF WebAssembly X86\"";
        let package_use = "*/* LLVM_TARGETS: -* AMDGPU BPF WebAssembly X86";
        let iuse = "keep llvm_targets_AArch64 llvm_targets_ARM llvm_targets_AMDGPU
            llvm_targets_BPF llvm_targets_WebAssembly llvm_targets_X86";
        let cases = [
            ("keep", true),
            ("llvm_targets_AArch64", false),
            ("llvm_targets_ARM", false),
            ("llvm_targets_AMDGPU", true),
            ("llvm_targets_BPF", true),
            ("llvm_targets_WebAssembly", true),
            ("llvm_targets_X86", true),
        ];

        for (flag, expected) in cases {
            assert_eq!(
                use_state(makenv, "", "", package_use, iuse, "", flag)?,
                Some(expected),
                "{flag}"
            );
        }
        Ok(())
    }

    #[test]
    fn test_package_use_reset_precedence() -> anyhow::Result<()> {
        let makenv = "USE_EXPAND=LLVM_TARGETS
            LLVM_TARGETS=\"NVPTX AMDGPU\"";
        let profile_use = "dev-lang/rust LLVM_TARGETS: NVPTX";
        let package_use = "*/* LLVM_TARGETS: -* AMDGPU";
        let iuse = "llvm_targets_NVPTX llvm_targets_AMDGPU";
        let cases = [("llvm_targets_NVPTX", false), ("llvm_targets_AMDGPU", true)];

        for (flag, expected) in cases {
            let state = use_state(makenv, "", profile_use, package_use, iuse, "", flag)?;
            assert_eq!(state, Some(expected), "{flag}");
        }
        Ok(())
    }

    #[test]
    fn test_package_use_order() -> anyhow::Result<()> {
        // Equal precedence across the exact and wildcard buckets: the later atom wins.
        let cases = [
            ("*/* foo\ndev-lang/rust -foo", false),
            ("dev-lang/rust -foo\n*/* foo", true),
            ("dev-lang/* -foo\ndev-lang/rust foo", true),
            ("dev-lang/rust foo\ndev-lang/* -foo", false),
            ("*/rust foo\ndev-lang/rust -foo", false),
            ("dev-lang/rust -foo\n*/rust foo", true),
        ];

        for (package_use, expected) in cases {
            assert_eq!(
                use_state("", "", "", package_use, "foo", "", "foo")?,
                Some(expected),
                "{package_use}"
            );
        }
        Ok(())
    }

    #[test]
    fn test_effective_use() {
        // flag, makenv, make_conf, profile_use, package_use, iuse, implicit, expected
        #[rustfmt::skip]
        let cases = [
            ("foo", "USE=foo", "", "", "*/* -foo", "+foo", "", Some(false)),
            ("foo", "", "USE=-foo", "", "*/* foo", "foo", "", Some(true)),
            ("foo", "USE=foo", "", "", "", "", "", None),
            ("foo", "", "", "", "", "+foo", "", Some(true)),
            ("foo", "", "USE=-foo", "", "", "+foo", "", Some(false)),
            // An implicit flag is available without appearing in IUSE; package.use
            // overrides the global state, otherwise it defaults to disabled.
            ("implicit", "", "USE=-implicit", "", "dev-lang/rust implicit", "", "implicit", Some(true)),
            ("implicit", "", "", "", "", "", "implicit", Some(false)),
        ];

        for (flag, makenv, make_conf, profile_use, package_use, iuse, implicit, expected) in cases {
            assert_eq!(
                use_state(
                    makenv,
                    make_conf,
                    profile_use,
                    package_use,
                    iuse,
                    implicit,
                    flag
                )
                .unwrap(),
                expected
            );
        }
    }
}
