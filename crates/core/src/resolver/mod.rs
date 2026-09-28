mod deps;
mod outcome;
mod plan;
mod policy;
mod provider;
mod state;
#[cfg(test)]
pub(crate) mod test_support;

use log::{debug, info, trace};

pub use self::deps::DependencyKind;
pub use self::outcome::{CandidateRejection, ResolutionOutcome, SelectedPackage};
pub use self::plan::{ExecutionPlan, PackageOperation};
pub use self::provider::{Candidate, PackageProvider};

use self::state::{PackageKey, ResolverState};
use crate::atom::Atom;
use crate::package::{AtomRequirement, Package};
use crate::policy::PackagePolicy;
use crate::resolver::policy::eval_policy;
use crate::resolver::provider::PackageLookup;
use crate::useflag::EffectiveUse;

/// Orchestrates dependency resolution for [`Package`]s.
pub struct Resolver<U> {
    provider: U,
    policy: PackagePolicy,
    state: ResolverState,
}

impl<U: PackageLookup> Resolver<U> {
    /// Creates a new resolver for the given [`PackageLookup`] and [`PackagePolicy`].
    pub fn new(provider: U, policy: PackagePolicy) -> Self {
        Self {
            provider,
            policy,
            state: ResolverState::default(),
        }
    }

    /// Resolves the given `atom` and returns the resolution outcome.
    pub async fn resolve(mut self, atom: &Atom) -> anyhow::Result<ResolutionOutcome> {
        info!("Resolving candidates for {atom}...");
        let is_resolved = self.resolve_candidates(AtomRequirement::root(atom)).await?;
        // TODO: don't create a plan when it couldn't be fully resolved
        Ok(self.state.finalize(is_resolved))
    }

    /// Resolves candidates for the given `atom` requirement.
    ///
    /// Returns `true` if a candidate was found, `false` otherwise.
    async fn resolve_candidates(&mut self, request: AtomRequirement<'_>) -> anyhow::Result<bool> {
        let pkgs = self.provider.repo_match_by_atom(request.atom()).await?;
        for candidate in eval_policy(pkgs, &self.policy) {
            match candidate {
                Candidate::Accepted(pkg, effective_use) => {
                    if self.eval_candidate(&request, pkg, effective_use).await? {
                        return Ok(true);
                    }
                }
                Candidate::Rejected(pkg, rejection) => {
                    self.state.reject(PackageKey::new(&pkg), rejection);
                }
            }
        }
        Ok(false)
    }

    /// Evaluates a package candidate including its dependencies.
    async fn eval_candidate(
        &mut self,
        request: &AtomRequirement<'_>,
        pkg: Package,
        effective_use: EffectiveUse,
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

        if !request.satisfied_by(&pkg, &effective_use)? {
            return Ok(false);
        }
        if self.state.is_blocked(&pkg, &effective_use)? {
            info!("skipping {pkg} due to active blocker");
            return Ok(false);
        }

        debug!("resolving dependencies for {pkg}");
        self.state
            .visiting(key.clone(), pkg.clone(), effective_use.clone());

        if let Some(kind) = self.resolve_dependencies(&pkg, &effective_use).await? {
            self.state.reject(
                PackageKey::new(&pkg),
                CandidateRejection::DependencyUnsatisfied(kind),
            );
            return Ok(false);
        }

        if let Some(installed) = self.provider.vdb_match_by_pkg(&pkg).await? {
            self.state.insert_replacement(key.clone(), installed);
        }
        self.state.select(key);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{TestResolver, installed};
    use super::*;
    use crate::test_support::pkg;

    #[tokio::test]
    async fn test_resolve_use_conditional_enabled() {
        let atom = "app-misc/root".parse().unwrap();
        let metadata = [
            ("IUSE", "feature"),
            ("DEPEND", "feature? ( app-misc/child )"),
        ];
        let root = pkg("app-misc", "root", "1", &metadata);
        let child = pkg("app-misc", "child", "1", &[]);

        let outcome = TestResolver::new([root, child])
            .with_use(&["feature"])
            .resolve(&atom)
            .await;
        assert!(outcome.is_resolved());
    }

    #[tokio::test]
    async fn test_resolve_use_conditional_disabled() {
        let atom = "app-misc/root".parse().unwrap();
        let metadata = [
            ("IUSE", "feature"),
            ("DEPEND", "feature? ( app-misc/child )"),
        ];
        let root = pkg("app-misc", "root", "1", &metadata);

        let outcome = TestResolver::new([root]).resolve(&atom).await;
        assert!(outcome.is_resolved());
    }

    #[tokio::test]
    async fn test_resolve_use_conditional_missing() {
        let atom = "app-misc/root".parse().unwrap();
        let metadata = [
            ("IUSE", "feature"),
            ("DEPEND", "feature? ( app-misc/child )"),
        ];
        let root = pkg("app-misc", "root", "1", &metadata);

        let outcome = TestResolver::new([root])
            .with_use(&["feature"])
            .resolve(&atom)
            .await;
        assert!(!outcome.is_resolved());
    }

    #[tokio::test]
    async fn test_resolve_fallback_masked() {
        let atom = "app-misc/root".parse().unwrap();
        let metadata = [("DEPEND", "app-misc/child")];
        let packages = [
            pkg("app-misc", "root", "1", &metadata),
            pkg("app-misc", "child", "2", &[]),
            pkg("app-misc", "child", "1", &[]),
        ];

        let outcome = TestResolver::new(packages)
            .with_mask("=app-misc/child-2")
            .resolve(&atom)
            .await;
        assert!(outcome.is_resolved());
    }

    #[tokio::test]
    async fn test_resolve_selected() {
        let atom = "app-misc/root".parse().unwrap();
        let metadata = [("DEPEND", "app-misc/child app-misc/child")];
        let candidates = [
            pkg("app-misc", "root", "1", &metadata),
            pkg("app-misc", "child", "1", &[]),
        ];

        let outcome = TestResolver::new(candidates).resolve(&atom).await;
        assert!(outcome.is_resolved());
    }

    #[tokio::test]
    async fn test_plan_merge() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg("app-misc", "root", "1", &[]);
        let outcome = TestResolver::new([root.clone()]).resolve(&atom).await;

        assert_eq!(
            outcome.plan().operations(),
            &[PackageOperation::Merge(SelectedPackage::new(
                root,
                EffectiveUse::default(),
            ))]
        );
    }

    #[tokio::test]
    async fn test_plan_replace() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg("app-misc", "root", "1", &[]);
        let installed = installed("root", "1", "0", &[], &[]);
        let outcome = TestResolver::new([root.clone()])
            .with_installed([installed.clone()])
            .resolve(&atom)
            .await;

        assert_eq!(
            outcome.plan().operations(),
            &[PackageOperation::Replace(
                SelectedPackage::new(root, EffectiveUse::default()),
                installed,
            )]
        );
    }

    #[tokio::test]
    async fn test_plan_upgrade() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg("app-misc", "root", "2", &[]);
        let installed = installed("root", "1", "0", &[], &[]);
        let outcome = TestResolver::new([root.clone()])
            .with_installed([installed.clone()])
            .resolve(&atom)
            .await;

        assert_eq!(
            outcome.plan().operations(),
            &[PackageOperation::Upgrade(
                SelectedPackage::new(root, EffectiveUse::default()),
                installed,
            )]
        );
    }

    #[tokio::test]
    async fn test_plan_downgrade() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg("app-misc", "root", "1", &[]);
        let installed = installed("root", "2", "0", &[], &[]);
        let outcome = TestResolver::new([root.clone()])
            .with_installed([installed.clone()])
            .resolve(&atom)
            .await;

        assert_eq!(
            outcome.plan().operations(),
            &[PackageOperation::Downgrade(
                SelectedPackage::new(root, EffectiveUse::default()),
                installed,
            )]
        );
    }

    #[tokio::test]
    async fn test_plan_other_slot_merge() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg("app-misc", "root", "1", &[]);
        let installed = installed("root", "1", "1", &[], &[]);
        let outcome = TestResolver::new([root.clone()])
            .with_installed([installed])
            .resolve(&atom)
            .await;

        assert_eq!(
            outcome.plan().operations(),
            &[PackageOperation::Merge(SelectedPackage::new(
                root,
                EffectiveUse::default(),
            ))]
        );
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_self() {
        let atom = "app-misc/root".parse().unwrap();
        let metadata = [("DEPEND", "!app-misc/root")];
        let result = TestResolver::new([pkg("app-misc", "root", "1", &metadata)])
            .resolve(&atom)
            .await;
        assert!(result.is_resolved());
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_selected() {
        let atom = "app-misc/root".parse().unwrap();
        let metadata = [("DEPEND", "app-misc/a app-misc/b")];
        let candidates = [
            pkg("app-misc", "root", "1", &metadata),
            pkg("app-misc", "a", "1", &[("DEPEND", "!app-misc/b")]),
            pkg("app-misc", "b", "1", &[]),
        ];
        let result = TestResolver::new(candidates).resolve(&atom).await;
        assert!(!result.is_resolved());
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_removal() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg("app-misc", "root", "1", &[("DEPEND", "!app-misc/other")]);
        let other = installed("other", "1", "0", &[], &[]);
        let outcome = TestResolver::new([root.clone()])
            .with_installed([other.clone()])
            .resolve(&atom)
            .await;

        assert!(outcome.is_resolved());
        assert_eq!(
            outcome.plan().operations(),
            &[
                PackageOperation::Merge(SelectedPackage::new(root, EffectiveUse::default())),
                PackageOperation::Unmerge(other),
            ]
        );
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_removal_self() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg("app-misc", "root", "1", &[("DEPEND", "!app-misc/root")]);
        let installed_root = installed("root", "1", "0", &[], &[]);
        let outcome = TestResolver::new([root.clone()])
            .with_installed([installed_root.clone()])
            .resolve(&atom)
            .await;

        assert!(outcome.is_resolved());
        assert_eq!(
            outcome.plan().operations(),
            &[PackageOperation::Replace(
                SelectedPackage::new(root, EffectiveUse::default()),
                installed_root,
            )]
        );
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_removal_use_dep_disabled() {
        let atom = "app-misc/root".parse().unwrap();
        let metadata = [("DEPEND", "!app-misc/other[flag]")];
        let root = pkg("app-misc", "root", "1", &metadata);
        let other = installed("other", "1", "0", &["flag"], &[]);
        let outcome = TestResolver::new([root.clone()])
            .with_installed([other])
            .resolve(&atom)
            .await;

        assert!(outcome.is_resolved());
        assert_eq!(
            outcome.plan().operations(),
            &[PackageOperation::Merge(SelectedPackage::new(
                root,
                EffectiveUse::default(),
            ))]
        );
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_removal_use_dep_enabled() {
        let atom = "app-misc/root".parse().unwrap();
        let metadata = [("DEPEND", "!app-misc/other[flag]")];
        let root = pkg("app-misc", "root", "1", &metadata);

        let other = installed("other", "1", "0", &["flag"], &["flag"]);
        let outcome = TestResolver::new([root.clone()])
            .with_installed([other.clone()])
            .resolve(&atom)
            .await;

        assert!(outcome.is_resolved());
        assert_eq!(
            outcome.plan().operations(),
            &[
                PackageOperation::Merge(SelectedPackage::new(root, EffectiveUse::default())),
                PackageOperation::Unmerge(other),
            ]
        );
    }
}
