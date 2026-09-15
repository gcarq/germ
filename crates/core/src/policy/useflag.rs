use crate::deps::atom::Atom;
use crate::deps::expression::{Expression, ExpressionNode};
use crate::files::pkgfile::{PackageUseRecords, UseFlags};
use crate::files::{UseEntries, entry::Entry};
use crate::package::PackageView;
use crate::profile::ProfileUseRecords;
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::{IUseEntry, IUseState, UseExpandConfig, UseFlag};
use crate::utils::{Inherit, try_all, try_any};
use anyhow::{Context, bail};

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
        let use_state = self.effective_for(pkg, stable_in_use);
        let tree = pkg.metadata().required_use().view();
        // TODO: This currently only checks the required USE flags to satisfy the expression,
        // inactive branches can contain unreferenced USE flags which might not be PMS compatible.
        try_all(tree.roots(), |n| {
            eval_expression(n.expression(), &use_state)
        })
    }

    /// Returns the effective USE state for the given [`PackageView`].
    fn effective_for<'a, P: PackageView>(
        &'a self,
        pkg: &'a P,
        stable_in_use: bool,
    ) -> EffectiveUse<'a> {
        let available = pkg
            .metadata()
            .iuse()
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
                    .iuse()
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
struct EffectiveUse<'a> {
    /// All flags that can exist for the package.
    /// This corresponds to `IUSE_EFFECTIVE`.
    available: FxHashSet<&'a UseFlag>,
    /// Available flags that are enabled.
    enabled: FxHashSet<&'a UseFlag>,
}

impl<'a> EffectiveUse<'a> {
    /// Returns `true` if the given `flag` is enabled, `false` otherwise.
    /// `Err` is returned if the flag is not available.
    fn is_enabled(&self, flag: &UseFlag) -> anyhow::Result<bool> {
        match self.state(flag) {
            Some(enabled) => Ok(enabled),
            None => bail!("flag {flag} is not available"),
        }
    }

    /// Returns the effective state of the given `flag`.
    /// - `Some(true)` if the flag is enabled.
    /// - `Some(false)` if the flag is disabled.
    /// - `None` if the flag is not available.
    fn state(&self, flag: &UseFlag) -> Option<bool> {
        self.available
            .contains(flag)
            .then(|| self.enabled.contains(flag))
    }
}

/// Evaluates `expr` that is part of `REQUIRED_USE` to determine whether
/// the USE flags are satisfied for the given `state`.
fn eval_expression<'a>(
    expr: Expression<'a, UseFlag>,
    state: &EffectiveUse,
) -> anyhow::Result<bool> {
    let result = match expr {
        Expression::Item(flag) => state.is_enabled(flag)?,
        Expression::AllOf(nodes) => try_all(nodes, |n| eval_expression(n.expression(), state))?,
        Expression::AnyOf(nodes) => try_any(nodes, |n| eval_expression(n.expression(), state))?,
        Expression::ExactlyOneOf(nodes) => eval_up_to_two(nodes, state)? == 1,
        Expression::AtMostOneOf(nodes) => eval_up_to_two(nodes, state)? < 2,
        Expression::Use {
            flag,
            negated,
            nodes,
        } => match state.is_enabled(flag)? == negated {
            true => true,
            false => try_all(nodes, |n| eval_expression(n.expression(), state))?,
        },
        Expression::Not(node) => !eval_expression(node.expression(), state)?,
        _ => unreachable!(),
    };
    Ok(result)
}

/// Evaluates `nodes` and counts how many are satisfied by the given `state`.
///
/// Breaks early if more than two nodes are satisfied.
fn eval_up_to_two<'a, I>(nodes: I, state: &EffectiveUse) -> anyhow::Result<u8>
where
    I: IntoIterator<Item = ExpressionNode<'a, UseFlag>>,
{
    let mut count = 0;
    for node in nodes.into_iter() {
        if eval_expression(node.expression(), state)? {
            count += 1;
            if count > 2 {
                break;
            }
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deps::{DepExpression, ExpressionKind};
    use crate::eapi::Eapi;
    use crate::files::entry::Precedence;
    use crate::makenv::{MakeEnv, MakeEnvStack};
    use crate::package::Package;
    use crate::test_support::{cpv, package_metadata};

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
            package_metadata(&[("IUSE", iuse)]),
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
            ("!foo", false, false, true),
            ("!foo", true, false, false),
            ("foo bar", true, true, true),
            ("foo bar", true, false, false),
            ("( foo bar )", true, true, true),
            ("( foo bar )", true, false, false),
            ("|| ( foo bar )", false, true, true),
            ("|| ( foo bar )", false, false, false),
            ("^^ ( foo bar )", true, false, true),
            ("^^ ( foo bar )", true, true, false),
            ("?? ( foo bar )", true, false, true),
            ("?? ( foo bar )", true, true, false),
            ("foo? ( bar )", false, false, true),
            ("foo? ( bar )", true, true, true),
            ("foo? ( bar )", true, false, false),
            ("!foo? ( bar )", false, true, true),
            ("!foo? ( bar )", false, false, false),
            ("!foo? ( bar )", true, true, true),
            ("!foo? ( bar )", true, false, true),
        ];

        let foo = UseFlag::new("foo")?;
        let bar = UseFlag::new("bar")?;
        let available = FxHashSet::from_iter([&foo, &bar]);
        for (input, foo_state, bar_state, expected) in cases {
            let expr =
                DepExpression::<UseFlag>::parse(Eapi::Eight, ExpressionKind::RequiredUse, input)?;
            let state = EffectiveUse {
                available: available.clone(),
                enabled: [(&foo, foo_state), (&bar, bar_state)]
                    .into_iter()
                    .filter_map(|(flag, enabled)| enabled.then_some(flag))
                    .collect(),
            };
            let actual = try_all(expr.view().roots(), |n| {
                eval_expression(n.expression(), &state)
            })?;

            assert_eq!(actual, expected, "{input}");
        }
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
