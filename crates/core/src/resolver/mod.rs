mod installed;
mod outcome;
mod provider;
mod state;
#[cfg(test)]
pub(crate) mod test_support;

use std::fmt;

use log::{debug, info, trace, warn};

pub use self::installed::VdbPackages;
pub use self::outcome::{CandidateRejection, ResolutionOutcome, SelectedPackage};
pub use self::provider::{Candidate, PkgProvider, RepoPkgProvider};

use self::state::{PackageKey, ResolverState};
use crate::atom::{Atom, AtomBlocker};
use crate::deps::{AtomDep, ExprEval};
use crate::package::{AtomRequirement, Package, PackageView};
use crate::policy::PolicyResult;
use crate::useflag::{EffectiveUse, UseFlag};

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

pub struct Resolver<P, I> {
    provider: P,
    installed: I,
    state: ResolverState,
}

impl<P: PkgProvider, I: VdbPackages> Resolver<P, I> {
    pub fn new(provider: P, installed: I) -> Self {
        Self {
            provider,
            installed,
            state: ResolverState::default(),
        }
    }

    /// Resolves the given `atom` and returns the resolution outcome.
    pub async fn resolve(mut self, atom: &Atom) -> anyhow::Result<ResolutionOutcome> {
        info!("Resolving candidates for {atom}...");
        let resolved = self.resolve_candidates(AtomRequirement::root(atom)).await?;
        Ok(self.state.finalize(resolved))
    }

    /// Resolves candidates for the given `atom` requirement.
    ///
    /// Returns `true` if a candidate was found, `false` otherwise.
    async fn resolve_candidates(&mut self, request: AtomRequirement<'_>) -> anyhow::Result<bool> {
        for candidate in self.provider.candidates(request.atom()).await? {
            match candidate {
                Candidate::Evaluated { pkg, policy } => {
                    if self.eval_candidate(&request, pkg, policy).await? {
                        return Ok(true);
                    }
                }
                Candidate::Unavailable { cpv, error } => {
                    warn!("skipping unavailable candidate {cpv}: {error}");
                }
            }
        }
        Ok(false)
    }

    /// Handles blockers for the given `atom`.
    ///
    /// Returns `true` if the dependency can be satisfied, `false` if its blocked.
    async fn handle_blocker(
        &mut self,
        ctx: &DepContext<'_>,
        atom: &Atom,
        blocker: AtomBlocker,
    ) -> anyhow::Result<bool> {
        match blocker {
            AtomBlocker::Weak => {
                let req = AtomRequirement::dependency(atom, ctx.owner_use);
                for (_, sel) in self.state.selected().filter(|sel| &sel.1.pkg != ctx.owner) {
                    if sel.matches(&req)? {
                        warn!(
                            "{}\n\tblocker {atom} in {}\n\tmatched selected {}",
                            ctx.owner, ctx.kind, sel.pkg
                        );
                        return Ok(false);
                    }
                }

                for installed in self.installed.find_by_atom(atom).await? {
                    // Weak blockers against the same installed packages are ignored.
                    if installed.cpv() == ctx.owner.cpv() {
                        continue;
                    }
                    if !req.satisfied_by(&installed, installed.effective_use())? {
                        continue;
                    }
                    info!(
                        "{}: planning removal of {installed} for weak blocker {atom} in {}",
                        ctx.owner, ctx.kind
                    );
                    self.state.insert_removal(installed);
                }
            }
            AtomBlocker::Strong => {
                debug!(
                    "{}: cannot satisfy due to strong blocker {blocker}{atom} in {}",
                    ctx.owner, ctx.kind
                );
                return Ok(false);
            }
        };

        self.state
            .insert_blocker(ctx.owner_use.clone(), atom.clone());

        Ok(true)
    }

    /// Evaluates a package candidate including its dependencies.
    async fn eval_candidate(
        &mut self,
        request: &AtomRequirement<'_>,
        pkg: Package,
        policy: PolicyResult,
    ) -> anyhow::Result<bool> {
        let key = PackageKey::new(&pkg);

        if self.state.is_rejected(&key) {
            return Ok(false);
        }

        if self.state.is_selected(&key) || self.state.is_visiting(&key) {
            let matches = self.state.matches_key(&key, request)?;
            if matches {
                trace!("already selected: {pkg}");
            }
            return Ok(matches);
        }

        match policy {
            PolicyResult::Accepted(effective_use) => {
                if !request.satisfied_by(&pkg, &effective_use)? {
                    return Ok(false);
                }
                if self.state.is_blocked(&pkg, &effective_use)? {
                    info!("skipping {pkg} due to active blocker");
                    return Ok(false);
                }

                debug!("evaluating deps for {pkg}");
                self.state
                    .visiting(key.clone(), pkg.clone(), effective_use.clone());
                if self.eval_dependencies(&pkg, &effective_use).await.is_ok() {
                    self.state.select(key);
                    return Ok(true);
                }
            }
            PolicyResult::Masked => {
                self.state.reject(key, CandidateRejection::Masked);
            }
            PolicyResult::MissingKeyword => {
                self.state.reject(key, CandidateRejection::MissingKeyword);
            }
            PolicyResult::RequiredUseUnsatisfied => {
                self.state
                    .reject(key, CandidateRejection::RequiredUseUnsatisfied);
            }
        }
        Ok(false)
    }

    /// Evaluates all dependencies of a package.
    async fn eval_dependencies<'b>(
        &mut self,
        owner: &'b Package,
        owner_use: &'b EffectiveUse,
    ) -> anyhow::Result<(), CandidateRejection> {
        let dependencies = [
            (DependencyKind::Depend, owner.metadata().depend().view()),
            (DependencyKind::BDepend, owner.metadata().bdepend().view()),
            (DependencyKind::IDepend, owner.metadata().idepend().view()),
            (DependencyKind::RDepend, owner.metadata().rdepend().view()),
            (DependencyKind::PDepend, owner.metadata().pdepend().view()),
        ];
        for (kind, tree) in dependencies {
            let ctx = DepContext {
                owner,
                kind,
                owner_use,
            };
            let mut evaluator = DepEval {
                resolver: self,
                ctx: &ctx,
            };
            match tree.eval(&mut evaluator).await {
                Ok(true) => (),
                Ok(false) => Err(CandidateRejection::DependencyUnsatisfied(kind))?,
                Err(err) => Err(CandidateRejection::Err(err))?,
            }
        }
        Ok(())
    }
}

/// Evaluates dependency items for one package and dependency kind.
struct DepEval<'a, 'b, P, I> {
    resolver: &'a mut Resolver<P, I>,
    ctx: &'a DepContext<'b>,
}

impl<P: PkgProvider, I: VdbPackages> ExprEval<AtomDep> for DepEval<'_, '_, P, I> {
    async fn eval_item(&mut self, atom: &AtomDep) -> anyhow::Result<bool> {
        if let Some(blocker) = atom.blocker() {
            return self
                .resolver
                .handle_blocker(self.ctx, atom.inner(), blocker)
                .await;
        }
        let resolved = self
            .resolver
            .resolve_candidates(AtomRequirement::dependency(
                atom.inner(),
                self.ctx.owner_use,
            ))
            .await?;
        if !resolved {
            debug!(
                "rejected {}:\n\t -> unsatisfied {}: {atom}",
                self.ctx.owner, self.ctx.kind
            );
        }
        Ok(resolved)
    }

    fn is_use_enabled(&self, flag: &UseFlag) -> anyhow::Result<bool> {
        self.ctx.owner_use.is_enabled(flag)
    }
}

/// Context for evaluating a package dependency.
struct DepContext<'a> {
    /// Package currently being resolved.
    owner: &'a Package,
    owner_use: &'a EffectiveUse,
    kind: DependencyKind,
}

#[cfg(test)]
mod tests {
    use super::test_support::{TestPkg, TestPkgProvider, TestVdbPackages};
    use super::*;
    use crate::test_support::{cpv, pkg, pkg_metadata};
    use crate::useflag::test_support::effective;
    use crate::vdb::package::InstalledPackage;

    /// Returns an accepted `app-misc/<name>-1` candidate with the given metadata.
    fn candidate(name: &str, metadata: &[(&str, &str)]) -> TestPkg {
        TestPkg::new(
            pkg("app-misc", name, "1", metadata),
            PolicyResult::Accepted(EffectiveUse::default()),
        )
    }

    /// Returns an installed `app-misc/<name>-1` package with the given USE state.
    fn installed(name: &str, iuse_effective: &[&str], enabled: &[&str]) -> InstalledPackage {
        InstalledPackage::from_parts(
            cpv("app-misc", name, "1"),
            "gentoo".parse().unwrap(),
            pkg_metadata(&[]),
            effective(iuse_effective, enabled),
        )
    }

    /// Returns the `app-misc/root` candidate with the given `DEPEND`.
    fn root(depend: &str) -> TestPkg {
        candidate("root", &[("DEPEND", depend)])
    }

    /// Resolves `app-misc/root` over the given packages.
    async fn resolve(pkgs: impl IntoIterator<Item = TestPkg>) -> ResolutionOutcome {
        resolve_with(TestPkgProvider::new(pkgs), TestVdbPackages::default()).await
    }

    /// Resolves `app-misc/root` with the given `provider` and `vdb`.
    async fn resolve_with(provider: impl PkgProvider, vdb: impl VdbPackages) -> ResolutionOutcome {
        Resolver::new(provider, vdb)
            .resolve(&"app-misc/root".parse().unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_resolve_use_conditional() {
        let metadata = [
            ("IUSE", "feature"),
            ("DEPEND", "feature? ( app-misc/child )"),
        ];
        for (enabled, child_avail, expected) in [
            (false, false, true),
            (true, false, false),
            (true, true, true),
        ] {
            let root = TestPkg::new(
                pkg("app-misc", "root", "1", &metadata),
                PolicyResult::Accepted(effective(
                    &["feature"],
                    if enabled { &["feature"] } else { &[] },
                )),
            );
            let mut pkgs = vec![root];
            if child_avail {
                pkgs.push(candidate("child", &[]));
            }
            assert_eq!(resolve(pkgs).await.is_resolved(), expected);
        }
    }

    #[tokio::test]
    async fn test_resolve_fallback_masked() {
        let pkgs = [
            root("app-misc/child"),
            TestPkg::new(pkg("app-misc", "child", "2", &[]), PolicyResult::Masked),
            candidate("child", &[]),
        ];
        assert!(resolve(pkgs).await.is_resolved());
    }

    #[tokio::test]
    async fn test_resolve_fallback_unavailable() {
        let provider = TestPkgProvider::new([root("app-misc/child"), candidate("child", &[])])
            .with_failure("app-misc/child-2", "metadata resolution failed");
        let outcome = resolve_with(provider, TestVdbPackages::default()).await;
        assert!(outcome.is_resolved());
    }

    #[tokio::test]
    async fn test_resolve_selected() {
        let pkgs = [
            root("app-misc/child app-misc/child"),
            candidate("child", &[]),
        ];
        assert!(resolve(pkgs).await.is_resolved());
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_self() {
        assert!(resolve([root("!app-misc/root")]).await.is_resolved());
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_selected() {
        let pkgs = [
            root("app-misc/a app-misc/b"),
            candidate("a", &[("DEPEND", "!app-misc/b")]),
            candidate("b", &[]),
        ];
        assert!(!resolve(pkgs).await.is_resolved());
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_removal() {
        let installed = TestVdbPackages::new([installed("other", &[], &[])]);
        let outcome =
            resolve_with(TestPkgProvider::new([root("!app-misc/other")]), installed).await;

        assert!(outcome.is_resolved());
        assert_eq!(outcome.removals().len(), 1);
        assert_eq!(outcome.removals()[0].cpv(), &cpv("app-misc", "other", "1"));
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_removal_self() {
        let installed = TestVdbPackages::new([installed("root", &[], &[])]);
        let outcome = resolve_with(TestPkgProvider::new([root("!app-misc/root")]), installed).await;

        assert!(outcome.is_resolved());
        assert!(outcome.removals().is_empty());
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_removal_use_dep() {
        for (enabled, removals) in [(&[][..], 0), (&["flag"][..], 1)] {
            let installed = TestVdbPackages::new([installed("other", &["flag"], enabled)]);
            let provider = TestPkgProvider::new([root("!app-misc/other[flag]")]);
            let outcome = resolve_with(provider, installed).await;

            assert!(outcome.is_resolved());
            assert_eq!(outcome.removals().len(), removals);
        }
    }
}
