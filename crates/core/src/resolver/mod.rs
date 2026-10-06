mod candidate;
mod deps;
mod outcome;
mod plan;
mod provider;
mod speculative;
mod state;
#[cfg(test)]
pub(crate) mod test_support;

use germ_pms::Atom;
use log::info;

pub use self::outcome::{
    CandidateRejectionReason, RejectedCandidate, RequirementFailure, ResolutionOutcome,
};
pub use self::plan::{ExecutionPlan, PackageOperation};
pub use self::provider::{PackageLookup, PackageProvider};
use self::state::ResolverState;
use crate::package::{AtomRequirement, Package};
use crate::policy::PackagePolicy;
use crate::useflag::EffectiveUse;

/// Orchestrates dependency resolution for [`Package`](crate::package::Package)s.
///
/// Packages are fetched from the given [`PackageLookup`]
/// and evaluated against [`PackagePolicy`].
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

    /// Resolves the given [`Atom`] and returns a [`ResolutionOutcome`].
    pub async fn resolve(mut self, atom: &Atom) -> anyhow::Result<ResolutionOutcome> {
        info!("Resolving candidates for {atom}...");
        let failure = self.resolve_candidates(AtomRequirement::root(atom)).await?;
        // TODO: don't create a plan when it couldn't be fully resolved
        Ok(self.state.finalize(failure))
    }
}

/// A package together with the effective USE state it was resolved with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectivePackage {
    pub pkg: Package,
    pub effective_use: EffectiveUse,
}

impl EffectivePackage {
    pub const fn new(pkg: Package, effective_use: EffectiveUse) -> Self {
        Self { pkg, effective_use }
    }

    /// Returns true if the package satisfies the given [`AtomRequirement`].
    pub fn matches(&self, requirement: &AtomRequirement<'_>) -> anyhow::Result<bool> {
        requirement.satisfied_by(&self.pkg, &self.effective_use)
    }
}

#[cfg(test)]
mod tests {
    use germ_pms::DependencyField;

    use super::test_support::{ResolverFixture, installed};
    use super::*;
    use crate::policy::PolicyRejection;
    use crate::test_support::pkg;
    use crate::useflag::EffectiveUse;
    use crate::useflag::test_support::effective;

    /// Returns the candidate rejected behind the root `Exhausted` dependency failure.
    fn nested_rejection(outcome: &ResolutionOutcome) -> &RejectedCandidate {
        let Some(RequirementFailure::Exhausted(_, candidates)) = outcome.failure() else {
            panic!("expected an exhausted root candidate");
        };
        let [rejected] = candidates.as_slice() else {
            panic!("expected one rejected root candidate");
        };
        let CandidateRejectionReason::Dependency(_, failure) = &rejected.reason else {
            panic!("expected a dependency failure");
        };
        let RequirementFailure::Exhausted(_, candidates) = failure.as_ref() else {
            panic!("expected an exhausted dependency candidate");
        };
        let [rejected] = candidates.as_slice() else {
            panic!("expected one rejected dependency candidate");
        };
        rejected
    }

    #[tokio::test]
    async fn test_resolve_use_conditional() {
        let atom = "app-misc/root".parse().unwrap();
        let metadata = [
            ("IUSE", "feature"),
            ("DEPEND", "feature? ( app-misc/child )"),
        ];
        let root = || pkg("app-misc", "root", "1", &metadata);
        let child = pkg("app-misc", "child", "1", &[]);

        let enabled = ResolverFixture::new([root(), child])
            .with_use(&["feature"])
            .resolve(&atom)
            .await;
        assert!(enabled.is_resolved());

        let disabled = ResolverFixture::new([root()]).resolve(&atom).await;
        assert!(disabled.is_resolved());

        let unavailable = ResolverFixture::new([root()])
            .with_use(&["feature"])
            .resolve(&atom)
            .await;
        assert!(!unavailable.is_resolved());
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

        let outcome = ResolverFixture::new(packages)
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

        let outcome = ResolverFixture::new(candidates).resolve(&atom).await;
        assert!(outcome.is_resolved());
    }

    #[tokio::test]
    async fn test_resolve_dependency_cycle() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg("app-misc", "root", "1", &[("DEPEND", "app-misc/child")]);
        let child = pkg("app-misc", "child", "1", &[("DEPEND", "app-misc/root")]);
        let outcome = ResolverFixture::new([root.clone(), child.clone()])
            .resolve(&atom)
            .await;

        assert_eq!(
            outcome.plan().operations(),
            &[
                PackageOperation::Merge(EffectivePackage::new(child, EffectiveUse::default())),
                PackageOperation::Merge(EffectivePackage::new(root, EffectiveUse::default())),
            ]
        );
    }

    #[tokio::test]
    async fn test_failed_candidate_rollback_state() {
        let atom = "app-misc/root".parse().unwrap();
        let new_root = pkg(
            "app-misc",
            "root",
            "2",
            &[(
                "DEPEND",
                "|| ( app-misc/first-child app-misc/second-child ) !app-misc/fallback-child app-misc/missing",
            )],
        );
        let old_root = pkg(
            "app-misc",
            "root",
            "1",
            &[("DEPEND", "app-misc/fallback-child")],
        );
        let first = pkg("app-misc", "first-child", "1", &[]);
        let second = pkg("app-misc", "second-child", "1", &[]);
        let fallback = pkg("app-misc", "fallback-child", "1", &[]);
        let first_installed = installed("first-child", "0", "0", &[], &[]);
        let fallback_installed = installed("fallback-child", "1", "1", &[], &[]);
        let outcome =
            ResolverFixture::new([new_root, old_root.clone(), first, second, fallback.clone()])
                .with_installed([first_installed, fallback_installed])
                .resolve(&atom)
                .await;

        assert!(outcome.is_resolved());
        assert_eq!(
            outcome.plan().operations(),
            &[
                PackageOperation::Merge(EffectivePackage::new(fallback, EffectiveUse::default(),)),
                PackageOperation::Merge(EffectivePackage::new(old_root, EffectiveUse::default(),)),
            ]
        );
    }

    #[tokio::test]
    async fn test_plan_replace() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg("app-misc", "root", "1", &[]);
        let installed = installed("root", "1", "0", &[], &[]);
        let outcome = ResolverFixture::new([root.clone()])
            .with_installed([installed.clone()])
            .resolve(&atom)
            .await;

        assert_eq!(
            outcome.plan().operations(),
            &[PackageOperation::Replace(
                EffectivePackage::new(root, EffectiveUse::default()),
                installed,
            )]
        );
    }

    #[tokio::test]
    async fn test_plan_other_slot_merge() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg("app-misc", "root", "1", &[]);
        let installed = installed("root", "1", "1", &[], &[]);
        let outcome = ResolverFixture::new([root.clone()])
            .with_installed([installed])
            .resolve(&atom)
            .await;

        let pkg = EffectivePackage::new(root, EffectiveUse::default());
        assert_eq!(outcome.plan().operations(), &[PackageOperation::Merge(pkg)]);
    }

    #[tokio::test]
    async fn test_resolve_active_weak_blocker() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg(
            "app-misc",
            "root",
            "1",
            &[("DEPEND", "app-misc/a app-misc/b")],
        );
        let owner = pkg("app-misc", "a", "1", &[("DEPEND", "!app-misc/b")]);
        let blocked = pkg("app-misc", "b", "1", &[]);
        let blocked_atom = "app-misc/b".parse().unwrap();
        let outcome = ResolverFixture::new([root, owner.clone(), blocked.clone()])
            .resolve(&atom)
            .await;

        let rejected = nested_rejection(&outcome);
        assert_eq!(&rejected.pkg, &blocked);
        assert_eq!(
            rejected.reason,
            CandidateRejectionReason::ActiveBlocker {
                atom: blocked_atom,
                owner: Box::new(owner),
            }
        );
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_selected() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg(
            "app-misc",
            "root",
            "1",
            &[("DEPEND", "app-misc/b app-misc/a")],
        );
        let owner = pkg(
            "app-misc",
            "a",
            "1",
            &[("IUSE", "feature"), ("DEPEND", "!app-misc/b[feature?]")],
        );
        let conflict = pkg("app-misc", "b", "1", &[("IUSE", "feature")]);
        let blocked_atom = "app-misc/b[feature?]".parse().unwrap();
        let outcome = ResolverFixture::new([root, owner.clone(), conflict.clone()])
            .with_use(&["feature"])
            .resolve(&atom)
            .await;

        let rejected = nested_rejection(&outcome);
        assert_eq!(&rejected.pkg, &owner);
        assert_eq!(
            rejected.reason,
            CandidateRejectionReason::Dependency(
                DependencyField::Depend,
                Box::new(RequirementFailure::WeakBlocker {
                    atom: blocked_atom,
                    conflict: Box::new(conflict),
                }),
            )
        );
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_visiting() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg("app-misc", "root", "1", &[("DEPEND", "app-misc/child")]);
        let child = pkg("app-misc", "child", "1", &[("DEPEND", "!app-misc/root")]);
        let blocked_atom = "app-misc/root".parse().unwrap();
        let outcome = ResolverFixture::new([root.clone(), child.clone()])
            .resolve(&atom)
            .await;

        let rejected = nested_rejection(&outcome);
        assert_eq!(&rejected.pkg, &child);
        assert_eq!(
            rejected.reason,
            CandidateRejectionReason::Dependency(
                DependencyField::Depend,
                Box::new(RequirementFailure::WeakBlocker {
                    atom: blocked_atom,
                    conflict: Box::new(root),
                }),
            )
        );
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_removal() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg("app-misc", "root", "1", &[("DEPEND", "!app-misc/other")]);
        let other = installed("other", "1", "0", &[], &[]);
        let outcome = ResolverFixture::new([root.clone()])
            .with_installed([other.clone()])
            .resolve(&atom)
            .await;

        assert!(outcome.is_resolved());
        assert_eq!(
            outcome.plan().operations(),
            &[
                PackageOperation::Merge(EffectivePackage::new(root, EffectiveUse::default())),
                PackageOperation::Unmerge(other),
            ]
        );
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_removal_self() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg("app-misc", "root", "1", &[("DEPEND", "!app-misc/root")]);
        let installed = installed("root", "1", "0", &[], &[]);
        let outcome = ResolverFixture::new([root.clone()])
            .with_installed([installed.clone()])
            .resolve(&atom)
            .await;

        let pkg = EffectivePackage::new(root, EffectiveUse::default());
        assert!(outcome.is_resolved());
        assert_eq!(
            outcome.plan().operations(),
            &[PackageOperation::Replace(pkg, installed,)]
        );
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_removal_use_dep() {
        let atom = "app-misc/root".parse().unwrap();
        let metadata = [("DEPEND", "!app-misc/other[flag]")];
        let root = pkg("app-misc", "root", "1", &metadata);

        let disabled = installed("other", "1", "0", &["flag"], &[]);
        let outcome = ResolverFixture::new([root.clone()])
            .with_installed([disabled])
            .resolve(&atom)
            .await;

        let pkg = EffectivePackage::new(root.clone(), EffectiveUse::default());
        assert!(outcome.is_resolved());
        assert_eq!(outcome.plan().operations(), &[PackageOperation::Merge(pkg)]);

        let enabled = installed("other", "1", "0", &["flag"], &["flag"]);
        let outcome = ResolverFixture::new([root.clone()])
            .with_installed([enabled.clone()])
            .resolve(&atom)
            .await;

        let pkg = EffectivePackage::new(root, EffectiveUse::default());
        assert!(outcome.is_resolved());
        assert_eq!(
            outcome.plan().operations(),
            &[
                PackageOperation::Merge(pkg),
                PackageOperation::Unmerge(enabled),
            ]
        );
    }

    #[tokio::test]
    async fn test_resolve_weak_blocker_removal_owner_use() {
        let atom = "app-misc/root".parse().unwrap();
        let metadata = [("IUSE", "feature"), ("DEPEND", "!app-misc/other[feature?]")];
        let root = pkg("app-misc", "root", "1", &metadata);
        let enabled = installed("other", "2", "0", &["feature"], &["feature"]);
        let disabled = installed("other", "1", "0", &["feature"], &[]);
        let outcome = ResolverFixture::new([root.clone()])
            .with_use(&["feature"])
            .with_installed([disabled, enabled.clone()])
            .resolve(&atom)
            .await;

        let pkg = EffectivePackage::new(root, effective(&["feature"], &["feature"]));
        assert!(outcome.is_resolved());
        assert_eq!(
            outcome.plan().operations(),
            &[
                PackageOperation::Merge(pkg),
                PackageOperation::Unmerge(enabled),
            ]
        );
    }

    #[tokio::test]
    async fn test_resolve_missing_dependency_candidate() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg("app-misc", "root", "1", &[("DEPEND", "app-misc/missing")]);
        let missing = "app-misc/missing".parse().unwrap();
        let outcome = ResolverFixture::new([root.clone()]).resolve(&atom).await;
        let expected = RequirementFailure::Exhausted(
            atom,
            vec![RejectedCandidate::new(
                root,
                CandidateRejectionReason::Dependency(
                    DependencyField::Depend,
                    RequirementFailure::NoCandidate(missing).into(),
                ),
            )],
        );

        assert_eq!(outcome.failure(), Some(&expected));
        assert!(outcome.rejected().is_empty());
    }

    #[tokio::test]
    async fn test_resolve_missing_root_candidate() {
        let atom = "app-misc/missing".parse().unwrap();
        let outcome = ResolverFixture::new([]).resolve(&atom).await;

        assert_eq!(
            outcome.failure(),
            Some(&RequirementFailure::NoCandidate(atom))
        );
    }

    #[tokio::test]
    async fn test_resolve_rejected_dependency_candidates() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg("app-misc", "root", "1", &[("DEPEND", "app-misc/child")]);
        let child = pkg("app-misc", "child", "1", &[]);
        let outcome = ResolverFixture::new([root.clone(), child.clone()])
            .with_mask("app-misc/child")
            .resolve(&atom)
            .await;
        let expected = RequirementFailure::Exhausted(
            atom,
            vec![RejectedCandidate::new(
                root,
                CandidateRejectionReason::Dependency(
                    DependencyField::Depend,
                    Box::new(RequirementFailure::Exhausted(
                        "app-misc/child".parse().unwrap(),
                        vec![RejectedCandidate::new(
                            child,
                            CandidateRejectionReason::Policy(PolicyRejection::Masked),
                        )],
                    )),
                ),
            )],
        );

        assert_eq!(outcome.failure(), Some(&expected));
    }

    #[tokio::test]
    async fn test_resolve_strong_block() {
        let atom = "app-misc/root".parse().unwrap();
        let root = pkg("app-misc", "root", "1", &[("DEPEND", "!!app-misc/blocked")]);
        let blocked = "app-misc/blocked".parse().unwrap();
        let outcome = ResolverFixture::new([root.clone()]).resolve(&atom).await;
        let expected = RequirementFailure::Exhausted(
            atom,
            vec![RejectedCandidate::new(
                root,
                CandidateRejectionReason::Dependency(
                    DependencyField::Depend,
                    RequirementFailure::StrongBlocker(blocked).into(),
                ),
            )],
        );

        assert_eq!(outcome.failure(), Some(&expected));
    }

    #[tokio::test]
    async fn test_resolve_any_of_failures() {
        let atom = "app-misc/root".parse().unwrap();
        let metadata = [("DEPEND", "|| ( app-misc/first app-misc/second )")];
        let root = pkg("app-misc", "root", "1", &metadata);
        let first = "app-misc/first".parse().unwrap();
        let second = "app-misc/second".parse().unwrap();
        let outcome = ResolverFixture::new([root.clone()]).resolve(&atom).await;
        let expected = RequirementFailure::Exhausted(
            atom,
            vec![RejectedCandidate::new(
                root,
                CandidateRejectionReason::Dependency(
                    DependencyField::Depend,
                    Box::new(RequirementFailure::AnyOf(vec![
                        RequirementFailure::NoCandidate(first),
                        RequirementFailure::NoCandidate(second),
                    ])),
                ),
            )],
        );

        assert_eq!(outcome.failure(), Some(&expected));
    }

    #[tokio::test]
    async fn test_resolve_any_of_rollback_removal() {
        let atom = "app-misc/root".parse().unwrap();
        let metadata = [(
            "DEPEND",
            "|| ( ( app-misc/abandoned !app-misc/child app-misc/missing ) app-misc/child )",
        )];
        let root = pkg("app-misc", "root", "1", &metadata);
        let abandoned = pkg("app-misc", "abandoned", "1", &[]);
        let child = pkg("app-misc", "child", "1", &[]);
        let abandoned_installed = installed("abandoned", "0", "0", &[], &[]);
        let child_installed = installed("child", "1", "1", &[], &[]);
        let outcome = ResolverFixture::new([root.clone(), abandoned, child.clone()])
            .with_installed([abandoned_installed, child_installed])
            .resolve(&atom)
            .await;

        assert!(outcome.is_resolved());
        assert_eq!(
            outcome.plan().operations(),
            &[
                PackageOperation::Merge(EffectivePackage::new(child, EffectiveUse::default())),
                PackageOperation::Merge(EffectivePackage::new(root, EffectiveUse::default())),
            ]
        );
    }
}
