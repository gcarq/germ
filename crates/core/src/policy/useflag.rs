use anyhow::Context;
use germ_pms::deps::{ExprEval, RequiredUseFlag};
use germ_pms::{IUseState, PackageView, UseFlag};

use super::index::AtomIndex;
use crate::files::UseEntries;
use crate::files::entry::Entry;
use crate::files::pkgfile::{MatchedUse, PackageUseRecords, UseFlags};
use crate::profile::ProfileUseRecords;
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::{EffectiveUse, UseExpandConfig};
use crate::utils::Inherit;

/// Holds the result of evaluating a [`PackageView`]
/// against the effective USE flags.
#[derive(Debug, PartialEq, Eq)]
pub enum UseFlagEvalResult {
    Satisfied(EffectiveUse),
    Unsatisfied,
}

/// Simple DTO used to build [`UsePolicy`] from user configured USE entries.
#[derive(Default)]
pub struct LocalUseRecords {
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
    package_use: PackageUse,
    mask: UseRestriction,
    force: UseRestriction,
    expand_config: UseExpandConfig,
}

impl UsePolicy {
    /// Builds [`UsePolicy`] from global, profile, and local records.
    pub fn new(
        global_use: GlobalUseRules,
        iuse_implicit: FxHashSet<UseFlag>,
        profile: ProfileUseRecords,
        local: LocalUseRecords,
    ) -> anyhow::Result<Self> {
        let package_use = PackageUse::new(
            local.package_use.inherit(&profile.package_use)?,
            &profile.expand_config,
        )
        .context("failed to resolve package.use")?;
        let mask = UseRestriction {
            global: compile_global(local.use_mask.inherit(&profile.use_mask)?),
            stable_global: compile_global(profile.use_stable_mask),
            package: PackageUse::new(
                local.package_use_mask.inherit(&profile.package_use_mask)?,
                &profile.expand_config,
            )
            .context("failed to resolve package.use.mask")?,
            stable_package: PackageUse::new(
                profile.package_use_stable_mask,
                &profile.expand_config,
            )
            .context("failed to resolve package.use.stable.mask")?,
        };
        let force = UseRestriction {
            global: compile_global(profile.use_force),
            stable_global: compile_global(profile.use_stable_force),
            package: PackageUse::new(
                local
                    .package_use_force
                    .inherit(&profile.package_use_force)?,
                &profile.expand_config,
            )
            .context("failed to resolve package.use.force")?,
            stable_package: PackageUse::new(
                profile.package_use_stable_force,
                &profile.expand_config,
            )
            .context("failed to resolve package.use.stable.force")?,
        };

        Ok(Self {
            global_use,
            iuse_implicit,
            package_use,
            mask,
            force,
            expand_config: profile.expand_config,
        })
    }

    /// Evaluates the required USE flags and returns [`UseFlagEvalResult`].
    ///
    /// TODO: This currently only checks the required USE flags to satisfy the expression,
    ///       inactive branches can contain unreferenced USE flags which might not be PMS compatible.
    pub fn eval<P>(&self, pkg: &P, stable_in_use: bool) -> anyhow::Result<UseFlagEvalResult>
    where
        P: PackageView,
    {
        let effective_use = self.effective_for(pkg, stable_in_use);
        let mut evaluator = ReqUseEval {
            state: &effective_use,
        };
        let satisfied = pkg.metadata().required_use().view().eval(&mut evaluator)?;
        Ok(match satisfied {
            true => UseFlagEvalResult::Satisfied(effective_use),
            false => UseFlagEvalResult::Unsatisfied,
        })
    }

    /// Returns the effective USE state for the given [`PackageView`].
    fn effective_for<P>(&self, pkg: &P, stable_in_use: bool) -> EffectiveUse
    where
        P: PackageView,
    {
        let pkg_use = self.package_use.matched_for(pkg);
        let masked = self.mask.matched_for(pkg, stable_in_use);
        let forced = self.force.matched_for(pkg, stable_in_use);

        // Explicit IUSE entries overwrite implicit ones.
        let iuse = pkg.metadata().iuse().iter();
        let iuse = self
            .iuse_implicit
            .iter()
            .map(|flag| (flag, None))
            .chain(iuse.map(|entry| (entry.flag(), entry.state())));

        let flags = iuse
            .map(|(flag, default)| {
                let enabled = !masked.state(flag).unwrap_or(false)
                    && (forced.state(flag).unwrap_or(false)
                        || pkg_use
                            .state(flag, &self.expand_config)
                            .or_else(|| self.global_use.state(flag))
                            .or_else(|| default.map(IUseState::as_bool))
                            .unwrap_or(false));
                (flag.clone(), enabled)
            })
            .collect();

        EffectiveUse::new(flags)
    }
}

/// Compiled USE mask or force rules from all sources.
///
/// Global rules are compiled once from the inherited records, retaining
/// negative operations; package rules stay atom-scoped and are matched
/// per package.
struct UseRestriction {
    global: FxHashMap<UseFlag, Entry<UseFlag>>,
    stable_global: FxHashMap<UseFlag, Entry<UseFlag>>,
    package: PackageUse,
    stable_package: PackageUse,
}

impl UseRestriction {
    /// Returns the restriction rules matched for the given `pkg`.
    fn matched_for<'a, P: PackageView>(
        &'a self,
        pkg: &P,
        stable_in_use: bool,
    ) -> MatchedRestriction<'a> {
        MatchedRestriction {
            restriction: self,
            package: self.package.matched_for(pkg),
            stable_package: stable_in_use.then(|| self.stable_package.matched_for(pkg)),
        }
    }
}

/// Compiles the inherited `entries` into global rules, retaining negative operations.
fn compile_global(entries: UseEntries) -> FxHashMap<UseFlag, Entry<UseFlag>> {
    entries
        .into_entries()
        .map(|entry| (entry.inner().clone(), entry))
        .collect()
}

/// The source of a USE restriction rule.
///
/// At equal precedence, package rules beat global rules and stable rules
/// beat base rules.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RuleSource {
    Global,
    GlobalStable,
    Package,
    PackageStable,
}

/// A [`UseRestriction`] matched for a single package.
///
/// `stable_package` being `None` means no stable rules are active,
/// including stable global rules.
struct MatchedRestriction<'a> {
    restriction: &'a UseRestriction,
    package: MatchedUse<'a>,
    stable_package: Option<MatchedUse<'a>>,
}

impl MatchedRestriction<'_> {
    /// Returns whether the winning rule sets the flag, if a rule applies.
    ///
    /// Candidates are ranked by precedence first, then by [`RuleSource`];
    /// an explicit negative rule is a winning unset, not the absence of a rule.
    fn state(&self, flag: &UseFlag) -> Option<bool> {
        let stable = self.stable_package.as_ref();
        let stable_global = stable.and_then(|_| self.restriction.stable_global.get(flag));
        let stable_package = stable.and_then(|matched| matched.entry(flag));

        [
            (RuleSource::Global, self.restriction.global.get(flag)),
            (RuleSource::GlobalStable, stable_global),
            (RuleSource::Package, self.package.entry(flag)),
            (RuleSource::PackageStable, stable_package),
        ]
        .into_iter()
        .filter_map(|(source, entry)| entry.map(|entry| (source, entry)))
        .max_by_key(|(source, entry)| (entry.prec, *source))
        .map(|(_, entry)| entry.op.as_bool())
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
    use germ_pms::deps::{ParsedExpr, RequiredUse};
    use germ_pms::test_support::pkg;

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
        let local = LocalUseRecords {
            package_use: PackageUseRecords::from_lines(package_use.lines(), Precedence::User)?,
            ..Default::default()
        };
        let profile = ProfileUseRecords {
            package_use: PackageUseRecords::from_lines(
                profile_use.lines(),
                Precedence::Profile(0),
            )?,
            expand_config: UseExpandConfig::from_makenv(makenv_stack.makenv())?,
            ..Default::default()
        };
        let implicit = implicit
            .split_whitespace()
            .map(UseFlag::new)
            .collect::<anyhow::Result<_>>()?;
        let policy = UsePolicy::new(makenv_stack.global_use()?, implicit, profile, local)?;
        let package = pkg("dev-lang/rust-1.0", &[("IUSE", iuse)]);
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
        for (input, foo_state, bar_state, expected) in cases {
            let expr = ParsedExpr::<RequiredUse>::parse(input)?;
            let state = EffectiveUse::new(FxHashMap::from_iter([
                (foo.clone(), foo_state),
                (bar.clone(), bar_state),
            ]));
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
            use_mask: UseEntries::from_lines("elogind".lines(), Precedence::Profile(0))?,
            use_force: UseEntries::from_lines("forced".lines(), Precedence::Profile(0))?,
            ..Default::default()
        };
        let policy = UsePolicy::new(
            FxHashMap::from_iter([(elogind.clone(), true), (systemd.clone(), true)]).into(),
            FxHashSet::default(),
            profile,
            LocalUseRecords::default(),
        )?;
        let package = pkg("sys-libs/pam-1.0", &[("IUSE", "elogind systemd forced")]);
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
            package_use_force: PackageUseRecords::from_lines(
                "*/* LLVM_TARGETS: AArch64".lines(),
                Precedence::Profile(0),
            )?,
            expand_config,
            ..Default::default()
        };
        let package = pkg(
            "llvm-core/clang-23.1.2",
            &[("IUSE", "llvm_targets_AArch64")],
        );
        let forced = UsePolicy::new(
            use_rules.clone(),
            FxHashSet::default(),
            profile.clone(),
            LocalUseRecords::default(),
        )?;
        let effective = forced.effective_for(&package, false).state(&flag);
        assert_eq!(effective, Some(true));

        let local = LocalUseRecords {
            package_use_force: PackageUseRecords::from_lines(
                "*/* LLVM_TARGETS: -AArch64".lines(),
                Precedence::User,
            )?,
            ..Default::default()
        };
        let unforced = UsePolicy::new(use_rules, FxHashSet::default(), profile, local)?;
        let effective = unforced.effective_for(&package, false).state(&flag);
        assert_eq!(effective, Some(false));
        Ok(())
    }

    #[test]
    fn test_profile_use_unmask() -> anyhow::Result<()> {
        let elogind = UseFlag::new("elogind")?;
        let masked = UseEntries::from_lines("elogind".lines(), Precedence::Profile(0))?;
        let profile = ProfileUseRecords {
            use_mask: UseEntries::from_lines("-elogind".lines(), Precedence::Profile(1))?
                .inherit(&masked)?,
            ..Default::default()
        };
        let policy = UsePolicy::new(
            FxHashMap::from_iter([(elogind.clone(), true)]).into(),
            FxHashSet::default(),
            profile,
            LocalUseRecords::default(),
        )?;
        let package = pkg("sys-libs/pam-1.0", &[("IUSE", "elogind")]);

        let effective = policy.effective_for(&package, false).state(&elogind);
        assert_eq!(effective, Some(true));
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
            let state = use_state(makenv, "", "", package_use, iuse, "", flag)?;
            assert_eq!(state, Some(expected), "{flag}");
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
            let state = use_state("", "", "", package_use, "foo", "", "foo")?;
            assert_eq!(state, Some(expected), "{package_use}");
        }
        Ok(())
    }

    #[test]
    fn test_effective_use() -> anyhow::Result<()> {
        // flag, makenv, make_conf, package_use, iuse, implicit, expected
        #[rustfmt::skip]
        let cases = [
            ("foo", "USE=foo", "", "*/* -foo", "+foo", "", Some(false)),
            ("foo", "", "USE=-foo", "*/* foo", "foo", "", Some(true)),
            ("foo", "USE=foo", "", "", "", "", None),
            ("foo", "", "", "", "+foo", "", Some(true)),
            ("foo", "", "USE=-foo", "", "+foo", "", Some(false)),
            // Explicit overrides implicit IUSE.
            ("foo", "", "", "", "+foo", "foo", Some(true)),
            // A bare duplicate does not cancel an explicit IUSE default (mingw64-toolchain).
            ("foo", "", "", "", "+foo foo", "", Some(true)),
            // An implicit flag is available without appearing in IUSE; package.use
            // overrides the global state, otherwise it defaults to disabled.
            ("implicit", "", "USE=-implicit", "*/* implicit", "", "implicit", Some(true)),
            ("implicit", "", "", "", "", "implicit", Some(false)),
        ];

        for (flag, makenv, make_conf, package_use, iuse, implicit, expected) in cases {
            let result = use_state(makenv, make_conf, "", package_use, iuse, implicit, flag)?;
            assert_eq!(result, expected);
        }
        Ok(())
    }

    fn restriction_policy(
        global_mask: Option<(&str, Precedence)>,
        global_force: Option<(&str, Precedence)>,
        pkg_mask: Option<(&str, Precedence)>,
        pkg_force: Option<(&str, Precedence)>,
    ) -> anyhow::Result<UsePolicy> {
        let entries = |line: Option<(&str, Precedence)>| match line {
            Some((line, prec)) => UseEntries::from_lines([line], prec),
            None => Ok(UseEntries::default()),
        };
        let records = |line: Option<(&str, Precedence)>| match line {
            Some((line, prec)) => PackageUseRecords::from_lines([line], prec),
            None => Ok(PackageUseRecords::default()),
        };
        let profile = ProfileUseRecords {
            use_mask: entries(global_mask)?,
            use_force: entries(global_force)?,
            ..Default::default()
        };
        let local = LocalUseRecords {
            package_use_mask: records(pkg_mask)?,
            package_use_force: records(pkg_force)?,
            ..Default::default()
        };
        UsePolicy::new(
            FxHashMap::from_iter([(UseFlag::new("foo")?, true)]).into(),
            FxHashSet::default(),
            profile,
            local,
        )
    }

    #[test]
    fn test_restriction_precedence() -> anyhow::Result<()> {
        let flag = UseFlag::new("foo")?;
        let package = pkg("dev-lang/rust-1.0", &[("IUSE", "foo")]);
        let p0 = Precedence::Profile(0);
        let p1 = Precedence::Profile(1);
        let user = Precedence::User;

        // global_mask, global_force, pkg_mask, pkg_force, mask, force, effective
        #[rustfmt::skip]
        let cases = [
            // A higher-precedence package unset cancels the global mask.
            (Some(("foo", p0)), None, Some(("dev-lang/rust -foo", user)), None, Some(false), None, Some(true)),
            // A higher-precedence profile unset cancels a lower package set.
            (Some(("-foo", p1)), None, Some(("dev-lang/rust foo", p0)), None, Some(false), None, Some(true)),
            // A higher-precedence package unset cancels the global force.
            (None, Some(("foo", p0)), None, Some(("dev-lang/rust -foo", user)), None, Some(false), Some(true)),
            // At equal precedence the package rule wins over the global rule.
            (Some(("foo", p0)), None, Some(("dev-lang/rust -foo", p0)), None, Some(false), None, Some(true)),
            // Mask wins over force for effective USE.
            (Some(("foo", p0)), Some(("foo", p0)), None, None, Some(true), Some(true), Some(false)),
            // No restriction preserves the global USE fallback.
            (None, None, None, None, None, None, Some(true)),
        ];

        for (
            global_mask,
            global_force,
            pkg_mask,
            pkg_force,
            expected_mask,
            expected_force,
            expected_effective,
        ) in cases
        {
            let policy = restriction_policy(global_mask, global_force, pkg_mask, pkg_force)?;
            let mask = policy.mask.matched_for(&package, false).state(&flag);
            let force = policy.force.matched_for(&package, false).state(&flag);
            let effective = policy.effective_for(&package, false).state(&flag);
            let expected = (expected_mask, expected_force, expected_effective);
            assert_eq!((mask, force, effective), expected);
        }
        Ok(())
    }

    #[test]
    fn test_restriction_stable_in_use() -> anyhow::Result<()> {
        let flag = UseFlag::new("foo")?;
        let profile = ProfileUseRecords {
            use_mask: UseEntries::from_lines(["foo"], Precedence::Profile(0))?,
            use_stable_mask: UseEntries::from_lines(["-foo"], Precedence::Profile(0))?,
            use_force: UseEntries::from_lines(["foo"], Precedence::Profile(0))?,
            use_stable_force: UseEntries::from_lines(["-foo"], Precedence::Profile(0))?,
            ..Default::default()
        };
        let policy = UsePolicy::new(
            FxHashMap::from_iter([(flag.clone(), true)]).into(),
            FxHashSet::default(),
            profile,
            LocalUseRecords::default(),
        )?;
        let package = pkg("dev-lang/rust-1.0", &[("IUSE", "foo")]);
        let mask = |stable| policy.mask.matched_for(&package, stable).state(&flag);
        let force = |stable| policy.force.matched_for(&package, stable).state(&flag);

        // Stable rules are inactive unless stable_in_use, then they win at equal precedence.
        let cases = [(false, true, true, false), (true, false, false, true)];
        for (stable, expected_mask, expected_force, expected_effective) in cases {
            let effective = policy.effective_for(&package, stable).state(&flag);
            assert_eq!(mask(stable), Some(expected_mask), "{stable}");
            assert_eq!(force(stable), Some(expected_force), "{stable}");
            assert_eq!(effective, Some(expected_effective), "{stable}");
        }
        Ok(())
    }

    #[test]
    fn test_restriction_reset_scope() -> anyhow::Result<()> {
        let flag = UseFlag::new("foo")?;
        let other = UseFlag::new("bar")?;
        let profile = ProfileUseRecords {
            use_mask: UseEntries::from_lines(["foo"], Precedence::Profile(0))?,
            ..Default::default()
        };
        let local = LocalUseRecords {
            package_use_mask: PackageUseRecords::from_lines(
                ["dev-lang/rust bar -*", "dev-libs/openssl foo"],
                Precedence::User,
            )?,
            ..Default::default()
        };
        let policy = UsePolicy::new(
            FxHashMap::from_iter([(flag.clone(), true), (other.clone(), true)]).into(),
            FxHashSet::default(),
            profile,
            local,
        )?;
        let rust = pkg("dev-lang/rust-1.0", &[("IUSE", "foo bar")]);
        let openssl = pkg("dev-libs/openssl-1.0", &[("IUSE", "foo bar")]);
        let masked = |package, flag| policy.mask.matched_for(package, false).state(flag);

        // A same-file `-*` reset clears the record's earlier entries, but the
        // global mask still applies to the atom and stays scoped to it.
        assert_eq!(masked(&rust, &flag), Some(true), "rust foo");
        assert_eq!(masked(&rust, &other), None, "rust bar");
        assert_eq!(masked(&openssl, &flag), Some(true), "openssl foo");
        Ok(())
    }
}
