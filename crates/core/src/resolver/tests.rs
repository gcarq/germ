use germ_pms::{BlockerStrength, DependencyField};

use super::test_support::{ResolverFixture, installed};
use super::*;
use crate::policy::PolicyRejection;
use crate::resolver::PackageOperation::{Merge, Replace, Unmerge, Upgrade};
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
        .resolve([&atom])
        .await;
    assert!(enabled.is_resolved());

    let disabled = ResolverFixture::new([root()]).resolve([&atom]).await;
    assert!(disabled.is_resolved());

    let unavailable = ResolverFixture::new([root()])
        .with_use(&["feature"])
        .resolve([&atom])
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
        .resolve([&atom])
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

    let outcome = ResolverFixture::new(candidates).resolve([&atom]).await;
    assert!(outcome.is_resolved());
}

#[tokio::test]
async fn test_resolve_dependency_cycle() {
    let atom = "app-misc/root".parse().unwrap();
    let root = pkg("app-misc", "root", "1", &[("DEPEND", "app-misc/child")]);
    let child = pkg("app-misc", "child", "1", &[("DEPEND", "app-misc/root")]);
    let outcome = ResolverFixture::new([root.clone(), child.clone()])
        .resolve([&atom])
        .await;

    assert_eq!(
        outcome.plan().operations(),
        &[
            Merge(EffectivePackage::new(child, EffectiveUse::default())),
            Merge(EffectivePackage::new(root, EffectiveUse::default())),
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
            .resolve([&atom])
            .await;

    assert!(outcome.is_resolved());
    assert_eq!(
        outcome.plan().operations(),
        &[
            Merge(EffectivePackage::new(fallback, EffectiveUse::default(),)),
            Merge(EffectivePackage::new(old_root, EffectiveUse::default(),)),
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
        .resolve([&atom])
        .await;

    assert_eq!(
        outcome.plan().operations(),
        &[Replace(
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
        .resolve([&atom])
        .await;

    let pkg = EffectivePackage::new(root, EffectiveUse::default());
    assert_eq!(outcome.plan().operations(), &[Merge(pkg)]);
}

#[tokio::test]
async fn test_resolve_active_blocker() {
    let atom = "app-misc/root".parse().unwrap();
    for prefix in ["!", "!!"] {
        let root = pkg(
            "app-misc",
            "root",
            "1",
            &[("DEPEND", "app-misc/a app-misc/b")],
        );
        let depend = format!("{prefix}app-misc/b");
        let owner = pkg("app-misc", "a", "1", &[("DEPEND", depend.as_str())]);
        let blocked = pkg("app-misc", "b", "1", &[]);
        let blocked_atom = "app-misc/b".parse().unwrap();
        let outcome = ResolverFixture::new([root, owner.clone(), blocked.clone()])
            .resolve([&atom])
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
}

#[tokio::test]
async fn test_resolve_blocker_in_graph() {
    for (prefix, strength) in [
        ("!", BlockerStrength::Weak),
        ("!!", BlockerStrength::Strong),
    ] {
        let atom = "app-misc/root".parse().unwrap();

        let depend = format!("{prefix}app-misc/b");
        let root = pkg(
            "app-misc",
            "root",
            "1",
            &[("DEPEND", "app-misc/b app-misc/a")],
        );
        let owner = pkg("app-misc", "a", "1", &[("DEPEND", depend.as_str())]);
        let conflict = pkg("app-misc", "b", "1", &[]);
        let outcome = ResolverFixture::new([root, owner.clone(), conflict.clone()])
            .resolve([&atom])
            .await;

        let rejected = nested_rejection(&outcome);
        assert_eq!(&rejected.pkg, &owner);
        assert_eq!(
            rejected.reason,
            CandidateRejectionReason::Dependency(
                DependencyField::Depend,
                Box::new(RequirementFailure::Blocker {
                    strength,
                    atom: "app-misc/b".parse().unwrap(),
                    conflict: Box::new(conflict),
                }),
            )
        );

        let depend = format!("{prefix}app-misc/root");
        let root = pkg("app-misc", "root", "1", &[("DEPEND", "app-misc/child")]);
        let child = pkg("app-misc", "child", "1", &[("DEPEND", depend.as_str())]);
        let outcome = ResolverFixture::new([root.clone(), child.clone()])
            .resolve([&atom])
            .await;

        let rejected = nested_rejection(&outcome);
        assert_eq!(&rejected.pkg, &child);
        assert_eq!(
            rejected.reason,
            CandidateRejectionReason::Dependency(
                DependencyField::Depend,
                Box::new(RequirementFailure::Blocker {
                    strength,
                    atom: "app-misc/root".parse().unwrap(),
                    conflict: Box::new(root),
                }),
            )
        );
    }
}

#[tokio::test]
async fn test_resolve_blocker_installed() {
    for (prefix, strength) in [
        ("!", BlockerStrength::Weak),
        ("!!", BlockerStrength::Strong),
    ] {
        let atom = "app-misc/root".parse().unwrap();
        let depend = format!("{prefix}app-misc/other");
        let root = pkg("app-misc", "root", "1", &[("DEPEND", depend.as_str())]);
        let other = installed("other", "1", "0", &[], &[]);
        let outcome = ResolverFixture::new([root.clone()])
            .with_installed([other.clone()])
            .resolve([&atom])
            .await;

        match strength {
            BlockerStrength::Weak => {
                assert!(outcome.is_resolved());
                assert_eq!(
                    outcome.plan().operations(),
                    &[
                        Merge(EffectivePackage::new(root, EffectiveUse::default(),)),
                        Unmerge(other),
                    ]
                );
            }
            BlockerStrength::Strong => {
                let expected = RequirementFailure::Exhausted(
                    atom,
                    vec![RejectedCandidate::new(
                        root,
                        CandidateRejectionReason::Dependency(
                            DependencyField::Depend,
                            Box::new(RequirementFailure::Blocker {
                                strength,
                                atom: "app-misc/other".parse().unwrap(),
                                conflict: Box::new(pkg("app-misc", "other", "1", &[])),
                            }),
                        ),
                    )],
                );
                assert_eq!(outcome.failure(), Some(&expected));
            }
        }
    }
}

#[tokio::test]
async fn test_resolve_blocker_same_slot() {
    let atom = "app-misc/root".parse().unwrap();
    for depend in ["!app-misc/root", "!!<app-misc/root-2"] {
        let root = pkg("app-misc", "root", "2", &[("DEPEND", depend)]);
        let installed = installed("root", "1", "0", &[], &[]);
        let outcome = ResolverFixture::new([root.clone()])
            .with_installed([installed.clone()])
            .resolve([&atom])
            .await;

        let pkg = EffectivePackage::new(root, EffectiveUse::default());
        assert!(outcome.is_resolved());
        assert_eq!(outcome.plan().operations(), &[Upgrade(pkg, installed)]);
    }
}

#[tokio::test]
async fn test_resolve_blocker_use() {
    let atom = "app-misc/root".parse().unwrap();

    let root = pkg(
        "app-misc",
        "root",
        "1",
        &[("DEPEND", "!app-misc/other[flag]")],
    );
    let disabled = installed("other", "1", "0", &["flag"], &[]);
    let outcome = ResolverFixture::new([root.clone()])
        .with_installed([disabled])
        .resolve([&atom])
        .await;
    let merged = EffectivePackage::new(root.clone(), EffectiveUse::default());
    assert!(outcome.is_resolved());
    assert_eq!(outcome.plan().operations(), &[Merge(merged)]);

    let enabled = installed("other", "1", "0", &["flag"], &["flag"]);
    let outcome = ResolverFixture::new([root.clone()])
        .with_installed([enabled.clone()])
        .resolve([&atom])
        .await;
    let merged = EffectivePackage::new(root.clone(), EffectiveUse::default());
    assert!(outcome.is_resolved());
    assert_eq!(
        outcome.plan().operations(),
        &[Merge(merged), Unmerge(enabled),]
    );

    let root = pkg(
        "app-misc",
        "root",
        "1",
        &[("IUSE", "feature"), ("DEPEND", "!app-misc/other[feature?]")],
    );
    let enabled = installed("other", "2", "0", &["feature"], &["feature"]);
    let disabled = installed("other", "1", "0", &["feature"], &[]);
    let outcome = ResolverFixture::new([root.clone()])
        .with_use(&["feature"])
        .with_installed([disabled, enabled.clone()])
        .resolve([&atom])
        .await;
    let merged = EffectivePackage::new(root, effective(&["feature"], &["feature"]));
    assert!(outcome.is_resolved());
    assert_eq!(
        outcome.plan().operations(),
        &[Merge(merged), Unmerge(enabled),]
    );
}

#[tokio::test]
async fn test_resolve_missing_dependency_candidate() {
    let atom = "app-misc/root".parse().unwrap();
    let root = pkg("app-misc", "root", "1", &[("DEPEND", "app-misc/missing")]);
    let missing = "app-misc/missing".parse().unwrap();
    let outcome = ResolverFixture::new([root.clone()]).resolve([&atom]).await;
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
    let outcome = ResolverFixture::new([]).resolve([&atom]).await;

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
        .resolve([&atom])
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
async fn test_resolve_strong_blocker() {
    let atom = "app-misc/root".parse().unwrap();
    let root = pkg("app-misc", "root", "1", &[("DEPEND", "!!app-misc/blocked")]);
    let outcome = ResolverFixture::new([root.clone()]).resolve([&atom]).await;

    assert!(outcome.is_resolved());
    assert_eq!(
        outcome.plan().operations(),
        &[Merge(EffectivePackage::new(root, EffectiveUse::default(),))]
    );
}

#[tokio::test]
async fn test_resolve_any_of_rollback_blocker() {
    for prefix in ["!", "!!"] {
        let atom = "app-misc/root".parse().unwrap();
        let depend = format!(
            "|| ( ( app-misc/abandoned {prefix}app-misc/child app-misc/missing ) app-misc/child )"
        );
        let root = pkg("app-misc", "root", "1", &[("DEPEND", depend.as_str())]);
        let abandoned = pkg("app-misc", "abandoned", "1", &[]);
        let child = pkg("app-misc", "child", "1", &[]);
        let abandoned_installed = installed("abandoned", "0", "0", &[], &[]);
        let child_installed = installed("child", "1", "1", &[], &[]);
        let outcome = ResolverFixture::new([root.clone(), abandoned, child.clone()])
            .with_installed([abandoned_installed, child_installed])
            .resolve([&atom])
            .await;

        assert!(outcome.is_resolved());
        assert_eq!(
            outcome.plan().operations(),
            &[
                Merge(EffectivePackage::new(child, EffectiveUse::default())),
                Merge(EffectivePackage::new(root, EffectiveUse::default())),
            ]
        );
    }
}

#[tokio::test]
async fn test_resolve_any_of_prefers_selected() {
    let atom = "app-misc/root".parse().unwrap();
    let metadata = [(
        "DEPEND",
        "app-misc/second || ( app-misc/first app-misc/second )",
    )];
    let root = pkg("app-misc", "root", "1", &metadata);
    let first = pkg("app-misc", "first", "1", &[]);
    let second = pkg("app-misc", "second", "1", &[]);
    let outcome = ResolverFixture::new([root.clone(), first, second.clone()])
        .resolve([&atom])
        .await;

    assert!(outcome.is_resolved());
    assert_eq!(
        outcome.plan().operations(),
        &[
            Merge(EffectivePackage::new(second, EffectiveUse::default())),
            Merge(EffectivePackage::new(root, EffectiveUse::default())),
        ]
    );
}

#[tokio::test]
async fn test_resolve_any_of_failures() {
    let atom = "app-misc/root".parse().unwrap();
    let metadata = [("DEPEND", "|| ( app-misc/first app-misc/second )")];
    let root = pkg("app-misc", "root", "1", &metadata);
    let first = "app-misc/first".parse().unwrap();
    let second = "app-misc/second".parse().unwrap();
    let outcome = ResolverFixture::new([root.clone()]).resolve([&atom]).await;
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
async fn test_resolve_multiple_roots() {
    let a = "app-misc/a".parse().unwrap();
    let b = "app-misc/b".parse().unwrap();
    let pkg_a = pkg("app-misc", "a", "1", &[]);
    let pkg_b = pkg("app-misc", "b", "1", &[]);
    let outcome = ResolverFixture::new([pkg_a.clone(), pkg_b.clone()])
        .resolve([&a, &b])
        .await;

    assert!(outcome.is_resolved());
    assert_eq!(
        outcome.plan().operations(),
        &[
            Merge(EffectivePackage::new(pkg_a, EffectiveUse::default())),
            Merge(EffectivePackage::new(pkg_b, EffectiveUse::default())),
        ]
    );
}
