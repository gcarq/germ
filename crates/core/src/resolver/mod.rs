pub mod result;
mod state;
#[cfg(test)]
pub(crate) mod test_support;

use std::fmt;

use anyhow::Context;
use futures_util::future::BoxFuture;
use log::{debug, info, warn};

use crate::atom::{Atom, AtomBlocker};
use crate::deps::AtomDep;
use crate::deps::expression::{Expression, ExpressionNodes};
use crate::package::cpv::CPV;
use crate::package::{AtomRequirement, Package, PackageView};
use crate::policy::{PackagePolicy, PolicyResult};
use crate::repository::{RepoName, RepoSet};
use crate::resolver::result::{CandidateRejection, CandidateResult};
use crate::resolver::state::ResolverState;
use crate::useflag::EffectiveUse;

/// Provides packages for dependency resolution.
pub trait PkgProvider: Send {
    /// Finds packages matching `atom`.
    fn find<'a>(
        &'a self,
        atom: &'a Atom,
    ) -> BoxFuture<'a, anyhow::Result<Vec<anyhow::Result<Package>>>>;

    /// Evaluates `pkg` against local package policy.
    fn evaluate(&self, pkg: &Package) -> anyhow::Result<PolicyResult>;
}

/// A package provider backed by a repository set.
pub struct RepoPkgProvider<'a> {
    reposet: &'a RepoSet,
    policy: &'a PackagePolicy,
}

impl<'a> RepoPkgProvider<'a> {
    pub const fn new(reposet: &'a RepoSet, policy: &'a PackagePolicy) -> Self {
        Self { reposet, policy }
    }
}

impl PkgProvider for RepoPkgProvider<'_> {
    fn find<'a>(
        &'a self,
        atom: &'a Atom,
    ) -> BoxFuture<'a, anyhow::Result<Vec<anyhow::Result<Package>>>> {
        Box::pin(async move {
            self.reposet
                .find_packages(atom)
                .await
                .map(|results| results.into_iter().map(|r| r.map_err(Into::into)).collect())
                .map_err(Into::into)
        })
    }

    fn evaluate(&self, pkg: &Package) -> anyhow::Result<PolicyResult> {
        self.policy.evaluate(pkg)
    }
}

/// Identifies the dependency kind being evaluated.
#[derive(Copy, Clone, Debug)]
pub enum DependencyKind {
    Depend,
    BDepend,
    IDepend,
    RDepend,
    PDepend,
}

impl fmt::Display for DependencyKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Depend => f.write_str("DEPEND"),
            Self::BDepend => f.write_str("BDEPEND"),
            Self::IDepend => f.write_str("IDEPEND"),
            Self::RDepend => f.write_str("RDEPEND"),
            Self::PDepend => f.write_str("PDEPEND"),
        }
    }
}

pub struct Resolver<Provider> {
    candidates: Provider,
    state: ResolverState,
}

impl<P: PkgProvider> Resolver<P> {
    pub fn new(candidates: P) -> Self {
        Self {
            candidates,
            state: ResolverState::default(),
        }
    }

    pub async fn resolve(&mut self, atom: &Atom) -> anyhow::Result<bool> {
        info!("Resolving candidates for {atom}...");
        self.resolve_candidates(AtomRequirement::root(atom)).await
    }

    async fn resolve_candidates(&mut self, request: AtomRequirement<'_>) -> anyhow::Result<bool> {
        for pkg in self.candidates.find(request.atom()).await? {
            let pkg = match pkg {
                Ok(pkg) => pkg,
                Err(err) => {
                    // TODO: format error
                    warn!("{err}");
                    continue;
                }
            };

            if self.eval_candidate(&request, &pkg).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Applies a blocker to the resolver state if it is not already satisfied.
    ///
    /// TODO: handle strong blockers
    fn apply_blocker(
        &mut self,
        ctx: &DepContext<'_>,
        atom: &Atom,
        blocker: AtomBlocker,
    ) -> anyhow::Result<bool> {
        let request = AtomRequirement::dependency(atom, ctx.owner_use);
        let key = PackageKey::new(ctx.owner);
        let exclude = blocker.is_weak().then_some(&key);

        if let Some(selected) = self.state.first_match(&request, exclude)? {
            warn!(
                "{}\n\tblocker {blocker}{atom} in {}\n\tmatched selected {selected}",
                ctx.owner, ctx.kind
            );
            return Ok(false);
        }

        self.state
            .insert_blocker(ctx.owner_use.clone(), atom.clone());

        Ok(true)
    }

    /// Evaluates a package candidate including its dependencies.
    async fn eval_candidate(
        &mut self,
        request: &AtomRequirement<'_>,
        pkg: &Package,
    ) -> anyhow::Result<bool> {
        let key = PackageKey::new(pkg);

        if self.state.is_rejected(&key) {
            return Ok(false);
        }

        if self.state.is_selected(&key) || self.state.is_visiting(&key) {
            let matches = self.state.matches_key(&key, request)?;
            if matches {
                debug!("already selected: {pkg}");
            }
            return Ok(matches);
        }

        match self.candidates.evaluate(pkg)? {
            PolicyResult::Accepted(effective_use) => {
                if !request.satisfied_by(pkg, &effective_use)? {
                    return Ok(false);
                }
                if self.state.is_blocked(pkg, &effective_use)? {
                    info!("skipping {pkg} due to active blocker");
                    return Ok(false);
                }

                debug!("evaluating deps for {pkg}");
                self.state
                    .visiting(key.clone(), pkg.clone(), effective_use.clone());
                match self.eval_dependencies(pkg, &effective_use).await {
                    CandidateResult::Selected => {
                        self.state.select(key);
                        return Ok(true);
                    }
                    CandidateResult::Rejected(reason) => {
                        // TODO: don't reject it globally,
                        // this might be valid in a different context.
                        self.state.reject(key);
                        warn!("skipping {pkg} due to {reason}");
                    }
                    CandidateResult::Err(err) => {
                        self.state.reject(key);
                        Err(err).with_context(|| format!("failed to evaluate candidate {pkg}"))?;
                    }
                }
            }
            PolicyResult::Masked => {
                self.state.reject(key);
                debug!("skipping {pkg} due to package mask");
            }
            PolicyResult::MissingKeyword => {
                self.state.reject(key);
                debug!("skipping {pkg} due to missing keyword");
            }
            PolicyResult::RequiredUseUnsatisfied => {
                self.state.reject(key);
                debug!("skipping {pkg} due to unsatisfied required USE flags");
            }
        }
        Ok(false)
    }

    /// Evaluates all dependencies of a package.
    async fn eval_dependencies<'b>(
        &mut self,
        owner: &'b Package,
        owner_use: &'b EffectiveUse,
    ) -> CandidateResult {
        let dependencies = [
            (DependencyKind::Depend, owner.metadata().depend().view()),
            (DependencyKind::BDepend, owner.metadata().bdepend().view()),
            (DependencyKind::IDepend, owner.metadata().idepend().view()),
            (DependencyKind::RDepend, owner.metadata().rdepend().view()),
            (DependencyKind::PDepend, owner.metadata().pdepend().view()),
        ];
        for (kind, exprtree) in dependencies {
            let ctx = DepContext {
                owner,
                kind,
                owner_use,
            };
            for node in exprtree.roots() {
                match self.eval_expression(&ctx, node.expression()).await {
                    Ok(true) => (),
                    Ok(false) => {
                        return CandidateResult::Rejected(
                            CandidateRejection::DependencyUnsatisfied(kind),
                        );
                    }
                    Err(err) => return CandidateResult::Err(err),
                }
            }
        }
        CandidateResult::Selected
    }

    /// Evaluates `expr` from a package dependency expression against its context.
    fn eval_expression<'b>(
        &'b mut self,
        ctx: &'b DepContext<'_>,
        expr: Expression<'b, AtomDep>,
    ) -> BoxFuture<'b, anyhow::Result<bool>> {
        Box::pin(async move {
            match expr {
                Expression::Item(atom) => {
                    if let Some(blocker) = atom.blocker() {
                        self.apply_blocker(ctx, atom.inner(), blocker)
                    } else {
                        let resolved = self
                            .resolve_candidates(AtomRequirement::dependency(
                                atom.inner(),
                                ctx.owner_use,
                            ))
                            .await?;
                        if !resolved {
                            debug!("{}: unsatisfied {} atom {atom}", ctx.owner, ctx.kind);
                        }
                        Ok(resolved)
                    }
                }
                Expression::AllOf(nodes) => {
                    for node in nodes {
                        if !self.eval_expression(ctx, node.expression()).await? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                Expression::AnyOf(nodes) => {
                    for node in nodes {
                        if self.eval_expression(ctx, node.expression()).await? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                Expression::ExactlyOneOf(nodes) => Ok(self.eval_up_to_two(ctx, nodes).await? == 1),
                Expression::AtMostOneOf(nodes) => Ok(self.eval_up_to_two(ctx, nodes).await? < 2),
                Expression::Use {
                    flag,
                    negated,
                    nodes,
                } => {
                    if ctx.owner_use.is_enabled(flag)? == negated {
                        return Ok(true);
                    }
                    debug!("evaluating USE expression for {flag}");
                    for node in nodes {
                        if !self.eval_expression(ctx, node.expression()).await? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
            }
        })
    }

    /// Evaluates `nodes` and counts how many are satisfied by the given context.
    ///
    /// Breaks early if more than two nodes are satisfied.
    async fn eval_up_to_two<'b>(
        &mut self,
        ctx: &'b DepContext<'_>,
        nodes: ExpressionNodes<'b, AtomDep>,
    ) -> anyhow::Result<u8> {
        let mut count = 0;
        for node in nodes {
            if self.eval_expression(ctx, node.expression()).await? {
                count += 1;
                if count > 2 {
                    break;
                }
            }
        }
        Ok(count)
    }
}

/// Context for evaluating a package dependency.
struct DepContext<'a> {
    /// Package currently being resolved.
    owner: &'a Package,
    owner_use: &'a EffectiveUse,
    kind: DependencyKind,
}

/// Represents a unique key for a package based on its CPV and repo name.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct PackageKey((CPV, RepoName));

impl PackageKey {
    fn new(pkg: &Package) -> Self {
        Self((pkg.cpv().clone(), pkg.repo().clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{TestPkg, TestPkgProvider};
    use super::*;
    use crate::test_support::pkg;
    use crate::useflag::test_support::effective;

    fn accepted(pkg: Package, available: &[&str], enabled: &[&str]) -> TestPkg {
        TestPkg::new(pkg, PolicyResult::Accepted(effective(available, enabled)))
    }

    /// Creates an accepted `app-misc/root` package with the given `DEPEND`.
    fn root(depend: &str) -> TestPkg {
        accepted(
            pkg("app-misc", "root", "1", &[("DEPEND", depend)]),
            &[],
            &[],
        )
    }

    async fn resolve(pkgs: impl IntoIterator<Item = TestPkg>, atom: &str) -> bool {
        Resolver::new(TestPkgProvider::new(pkgs))
            .resolve(&atom.parse().unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_resolve_use_conditional() {
        for (enabled, child_avail, expected) in [
            (false, false, true),
            (true, false, false),
            (true, true, true),
        ] {
            let root = pkg(
                "app-misc",
                "root",
                "1",
                &[
                    ("IUSE", "feature"),
                    ("DEPEND", "feature? ( app-misc/child )"),
                ],
            );
            let mut candidates = vec![accepted(
                root,
                &["feature"],
                if enabled { &["feature"] } else { &[] },
            )];
            if child_avail {
                candidates.push(accepted(pkg("app-misc", "child", "1", &[]), &[], &[]));
            }
            assert_eq!(resolve(candidates, "app-misc/root").await, expected);
        }
    }

    #[tokio::test]
    async fn test_resolve_candidate_fallback() {
        let result = resolve(
            [
                root("app-misc/child"),
                TestPkg::new(pkg("app-misc", "child", "2", &[]), PolicyResult::Masked),
                accepted(pkg("app-misc", "child", "1", &[]), &[], &[]),
            ],
            "app-misc/root",
        )
        .await;
        assert!(result);
    }

    #[tokio::test]
    async fn test_resolve_selected() {
        let result = resolve(
            [
                root("app-misc/child app-misc/child"),
                accepted(pkg("app-misc", "child", "1", &[]), &[], &[]),
            ],
            "app-misc/root",
        )
        .await;
        assert!(result);
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_self() {
        assert!(resolve([root("!app-misc/root")], "app-misc/root").await);
    }

    #[tokio::test]
    async fn test_resolve_blocker_selected() {
        let result = resolve(
            [
                root("app-misc/a app-misc/b"),
                accepted(
                    pkg("app-misc", "a", "1", &[("DEPEND", "!app-misc/b")]),
                    &[],
                    &[],
                ),
                accepted(pkg("app-misc", "b", "1", &[]), &[], &[]),
            ],
            "app-misc/root",
        )
        .await;
        assert!(!result);
    }
}
