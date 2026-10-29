use germ_pms::BlockerStrength;
use germ_pms::deps::{Depend, ExprKind};
use germ_pms::test_support::{atom, pkg};

use super::CandidateRejectionReason::{ActiveBlocker, Dependency, Policy, SlotConflict};
use super::RequirementFailure::{AnyOf, Blocker, Exhausted, NoCandidate};
use super::test_support::{ResolverFixture, installed};
use super::*;
use crate::files::test_support::origin;
use crate::policy::PolicyRejection;
use crate::resolver::PackageOperation::{Merge, Replace, Unmerge, Upgrade};
use crate::useflag::EffectiveUse;
use crate::useflag::test_support::effective;

/// Returns the candidate rejected behind the single root `Exhausted` dependency failure.
fn nested_rejection(outcome: &ResolutionOutcome) -> &RejectedCandidate {
    let [Exhausted(_, candidates)] = outcome.failures() else {
        panic!("expected an exhausted root candidate");
    };
    let [rejected] = candidates.as_slice() else {
        panic!("expected one rejected root candidate");
    };
    let Dependency(_, failure) = &rejected.reason else {
        panic!("expected a dependency failure");
    };
    let Exhausted(_, candidates) = failure.as_ref() else {
        panic!("expected an exhausted dependency candidate");
    };
    let [rejected] = candidates.as_slice() else {
        panic!("expected one rejected dependency candidate");
    };
    rejected
}

#[tokio::test]
async fn test_resolve_use_conditional() {
    let zlib_atom = atom("sys-libs/zlib");
    let atom = atom("app-editors/nano");
    let metadata = [
        ("IUSE", "feature"),
        ("DEPEND", "feature? ( sys-libs/zlib )"),
    ];
    let root = || pkg("app-editors/nano-2", &metadata);
    let child = pkg("sys-libs/zlib-2", &[]);

    let enabled = ResolverFixture::new([root(), child])
        .with_use(&["feature"])
        .resolve([&atom])
        .await;
    assert!(matches!(enabled, ResolutionOutcome::Complete { .. }));

    let disabled = ResolverFixture::new([root()]).resolve([&atom]).await;
    assert!(matches!(disabled, ResolutionOutcome::Complete { .. }));

    let unavailable = ResolverFixture::new([root()])
        .with_use(&["feature"])
        .resolve([&atom])
        .await;
    let expected = Exhausted(
        atom,
        vec![RejectedCandidate::new(
            root(),
            Dependency(Depend::VAR, NoCandidate(zlib_atom).into()),
        )],
    );
    assert_eq!(unavailable.failures(), &[expected]);
}

#[tokio::test]
async fn test_resolve_fallback_masked() {
    let atom = atom("app-editors/nano");
    let metadata = [("DEPEND", "sys-libs/zlib")];
    let root = pkg("app-editors/nano-2", &metadata);
    let zlib_2 = pkg("sys-libs/zlib-2", &[]);
    let zlib_1 = pkg("sys-libs/zlib-1", &[]);

    let outcome = ResolverFixture::new([root.clone(), zlib_2, zlib_1.clone()])
        .with_mask("=sys-libs/zlib-2")
        .resolve([&atom])
        .await;
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[
            Merge(EffectivePackage::new(zlib_1, EffectiveUse::default())),
            Merge(EffectivePackage::new(root, EffectiveUse::default())),
        ]
    );
}

#[tokio::test]
async fn test_resolve_selected() {
    let atom = atom("app-editors/nano");
    let zlib_2 = pkg("sys-libs/zlib-2", &[]);
    let zlib_1 = pkg("sys-libs/zlib-1", &[]);
    let cases = [
        ("sys-libs/zlib sys-libs/zlib", zlib_2.clone()),
        ("=sys-libs/zlib-1 sys-libs/zlib", zlib_1.clone()),
    ];

    for (depend, selected) in cases {
        let root = pkg("app-editors/nano-2", &[("DEPEND", depend)]);
        let outcome = ResolverFixture::new([root.clone(), zlib_2.clone(), zlib_1.clone()])
            .resolve([&atom])
            .await;

        assert!(
            matches!(outcome, ResolutionOutcome::Complete { .. }),
            "{depend}"
        );
        assert_eq!(
            outcome.plan().unwrap().operations(),
            &[
                Merge(EffectivePackage::new(selected, EffectiveUse::default())),
                Merge(EffectivePackage::new(root, EffectiveUse::default())),
            ],
            "{depend}"
        );
    }
}

#[tokio::test]
async fn test_resolve_dependency_cycle() {
    let atom = atom("app-editors/nano");
    let root = pkg("app-editors/nano-2", &[("DEPEND", "sys-libs/zlib")]);
    let child = pkg("sys-libs/zlib-2", &[("DEPEND", "app-editors/nano")]);
    let outcome = ResolverFixture::new([root.clone(), child.clone()])
        .resolve([&atom])
        .await;

    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[
            Merge(EffectivePackage::new(child, EffectiveUse::default())),
            Merge(EffectivePackage::new(root, EffectiveUse::default()))
        ]
    );
}

#[tokio::test]
async fn test_slot_conflict() {
    let older_atom = atom("=app/lib-1");
    let atom = atom("app-editors/nano");
    for (newer_slot, older_slot) in [("0", "0"), ("0/2", "0/1")] {
        let root = pkg("app-editors/nano-2", &[("DEPEND", "app/lib =app/lib-1")]);
        let newer = pkg("app/lib-2", &[("SLOT", newer_slot)]);
        let older = pkg("app/lib-1", &[("SLOT", older_slot)]);
        let outcome = ResolverFixture::new([root.clone(), newer.clone(), older.clone()])
            .resolve([&atom])
            .await;
        let expected = Exhausted(
            atom.clone(),
            vec![RejectedCandidate::new(
                root,
                Dependency(
                    Depend::VAR,
                    Exhausted(
                        older_atom.clone(),
                        vec![RejectedCandidate::new(
                            older,
                            SlotConflict {
                                selected: newer.into(),
                            },
                        )],
                    )
                    .into(),
                ),
            )],
        );

        assert_eq!(outcome.failures(), &[expected], "{newer_slot}");
    }
}

#[tokio::test]
async fn test_failed_candidate_rollback_state() {
    let atom = atom("app-editors/nano");
    let new_root = pkg(
        "app-editors/nano-2",
        &[(
            "DEPEND",
            "|| ( app-arch/gzip app-arch/bzip2 ) !app-arch/zstd dev-libs/libffi",
        )],
    );
    let old_root = pkg("app-editors/nano-1", &[("DEPEND", "app-arch/zstd")]);
    let first = pkg("app-arch/gzip-2", &[]);
    let second = pkg("app-arch/bzip2-1", &[]);
    let fallback = pkg("app-arch/zstd-1", &[]);
    let first_installed = installed("app-arch/gzip-1", "0", &[], &[]);
    let fallback_installed = installed("app-arch/zstd-1", "1", &[], &[]);
    let outcome =
        ResolverFixture::new([new_root, old_root.clone(), first, second, fallback.clone()])
            .with_installed([first_installed, fallback_installed])
            .resolve([&atom])
            .await;

    assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[
            Merge(EffectivePackage::new(fallback, EffectiveUse::default())),
            Merge(EffectivePackage::new(old_root, EffectiveUse::default()))
        ]
    );
}

#[tokio::test]
async fn test_plan_replace() {
    let atom = atom("app-editors/nano");
    let root = pkg("app-editors/nano-2", &[]);
    let installed = installed("app-editors/nano-2", "0", &[], &[]);
    let outcome = ResolverFixture::new([root.clone()])
        .with_installed([installed.clone()])
        .resolve([&atom])
        .await;

    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[Replace(
            EffectivePackage::new(root, EffectiveUse::default()),
            installed,
        )]
    );
}

#[tokio::test]
async fn test_plan_other_slot_merge() {
    let atom = atom("app-editors/nano");
    let root = pkg("app-editors/nano-2", &[]);
    let installed = installed("app-editors/nano-2", "1", &[], &[]);
    let outcome = ResolverFixture::new([root.clone()])
        .with_installed([installed])
        .resolve([&atom])
        .await;

    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[Merge(EffectivePackage::new(root, EffectiveUse::default()))]
    );
}

#[tokio::test]
async fn test_resolve_active_blocker() {
    let blocked_atom = atom("sys-apps/sed");
    let atom = atom("app-editors/nano");
    for prefix in ["!", "!!"] {
        let root = pkg(
            "app-editors/nano-2",
            &[("DEPEND", "sys-apps/grep sys-apps/sed")],
        );
        let depend = format!("{prefix}sys-apps/sed");
        let owner = pkg("sys-apps/grep-1", &[("DEPEND", depend.as_str())]);
        let blocked = pkg("sys-apps/sed-1", &[]);
        let outcome = ResolverFixture::new([root, owner.clone(), blocked.clone()])
            .resolve([&atom])
            .await;

        let rejected = nested_rejection(&outcome);
        assert_eq!(&rejected.pkg, &blocked);
        assert_eq!(
            rejected.reason,
            ActiveBlocker {
                atom: blocked_atom.clone(),
                owner: owner.into(),
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
        let sed_atom = atom("sys-apps/sed");
        let atom = atom("app-editors/nano");

        let depend = format!("{prefix}sys-apps/sed");
        let root = pkg(
            "app-editors/nano-2",
            &[("DEPEND", "sys-apps/sed sys-apps/grep")],
        );
        let owner = pkg("sys-apps/grep-1", &[("DEPEND", depend.as_str())]);
        let conflict = pkg("sys-apps/sed-1", &[]);
        let outcome = ResolverFixture::new([root, owner.clone(), conflict.clone()])
            .resolve([&atom])
            .await;

        let rejected = nested_rejection(&outcome);
        assert_eq!(&rejected.pkg, &owner);
        assert_eq!(
            rejected.reason,
            Dependency(
                Depend::VAR,
                Blocker {
                    strength,
                    atom: sed_atom,
                    conflict: conflict.into(),
                }
                .into(),
            )
        );

        let depend = format!("{prefix}app-editors/nano");
        let root = pkg("app-editors/nano-2", &[("DEPEND", "sys-libs/zlib")]);
        let child = pkg("sys-libs/zlib-2", &[("DEPEND", depend.as_str())]);
        let outcome = ResolverFixture::new([root.clone(), child.clone()])
            .resolve([&atom])
            .await;

        let rejected = nested_rejection(&outcome);
        assert_eq!(&rejected.pkg, &child);
        assert_eq!(
            rejected.reason,
            Dependency(
                Depend::VAR,
                Blocker {
                    strength,
                    atom: atom.clone(),
                    conflict: root.into(),
                }
                .into(),
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
        let pam_atom = atom("sys-libs/pam");
        let atom = atom("app-editors/nano");
        let depend = format!("{prefix}sys-libs/pam");
        let root = pkg("app-editors/nano-2", &[("DEPEND", depend.as_str())]);
        let other = installed("sys-libs/pam-2", "0", &[], &[]);
        let outcome = ResolverFixture::new([root.clone()])
            .with_installed([other.clone()])
            .resolve([&atom])
            .await;

        match strength {
            BlockerStrength::Weak => {
                assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
                assert_eq!(
                    outcome.plan().unwrap().operations(),
                    &[
                        Merge(EffectivePackage::new(root, EffectiveUse::default())),
                        Unmerge(other),
                    ]
                );
            }
            BlockerStrength::Strong => {
                let expected = Exhausted(
                    atom,
                    vec![RejectedCandidate::new(
                        root,
                        Dependency(
                            Depend::VAR,
                            Blocker {
                                strength,
                                atom: pam_atom,
                                conflict: pkg("sys-libs/pam-2", &[]).into(),
                            }
                            .into(),
                        ),
                    )],
                );
                assert_eq!(outcome.failures(), &[expected]);
            }
        }
    }
}

#[tokio::test]
async fn test_resolve_blocker_same_slot() {
    let atom = atom("app-editors/nano");
    for depend in ["!app-editors/nano", "!!<app-editors/nano-2"] {
        let root = pkg("app-editors/nano-2", &[("DEPEND", depend)]);
        let installed = installed("app-editors/nano-1", "0", &[], &[]);
        let outcome = ResolverFixture::new([root.clone()])
            .with_installed([installed.clone()])
            .resolve([&atom])
            .await;

        assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
        assert_eq!(
            outcome.plan().unwrap().operations(),
            &[Upgrade(
                EffectivePackage::new(root, EffectiveUse::default()),
                installed,
            )]
        );
    }
}

#[tokio::test]
async fn test_resolve_blocker_use() {
    let atom = atom("app-editors/nano");

    let root = pkg("app-editors/nano-2", &[("DEPEND", "!sys-libs/pam[flag]")]);
    let disabled = installed("sys-libs/pam-2", "0", &["flag"], &[]);
    let outcome = ResolverFixture::new([root.clone()])
        .with_installed([disabled])
        .resolve([&atom])
        .await;
    assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[Merge(EffectivePackage::new(
            root.clone(),
            EffectiveUse::default(),
        ))]
    );

    let enabled = installed("sys-libs/pam-2", "0", &["flag"], &["flag"]);
    let outcome = ResolverFixture::new([root.clone()])
        .with_installed([enabled.clone()])
        .resolve([&atom])
        .await;
    assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[
            Merge(EffectivePackage::new(root.clone(), EffectiveUse::default(),)),
            Unmerge(enabled),
        ]
    );

    let root = pkg(
        "app-editors/nano-2",
        &[("IUSE", "feature"), ("DEPEND", "!sys-libs/pam[feature?]")],
    );
    let enabled = installed("sys-libs/pam-2", "0", &["feature"], &["feature"]);
    let disabled = installed("sys-libs/pam-1", "0", &["feature"], &[]);
    let outcome = ResolverFixture::new([root.clone()])
        .with_use(&["feature"])
        .with_installed([disabled, enabled.clone()])
        .resolve([&atom])
        .await;
    let merged = EffectivePackage::new(root, effective(&["feature"], &["feature"]));
    assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[Merge(merged), Unmerge(enabled)]
    );
}

#[tokio::test]
async fn test_resolve_missing_dependency_candidate() {
    let missing = atom("dev-libs/libffi");
    let atom = atom("app-editors/nano");
    let root = pkg("app-editors/nano-2", &[("DEPEND", "dev-libs/libffi")]);
    let outcome = ResolverFixture::new([root.clone()]).resolve([&atom]).await;
    let expected = Exhausted(
        atom,
        vec![RejectedCandidate::new(
            root,
            Dependency(Depend::VAR, NoCandidate(missing).into()),
        )],
    );

    assert_eq!(outcome.failures(), &[expected]);
    assert!(outcome.rejected().is_empty());
}

#[tokio::test]
async fn test_resolve_rejected_dependency_candidates() {
    let zlib_atom = atom("sys-libs/zlib");
    let atom = atom("app-editors/nano");
    let root = pkg("app-editors/nano-2", &[("DEPEND", "sys-libs/zlib")]);
    let child = pkg("sys-libs/zlib-2", &[]);
    let outcome = ResolverFixture::new([root.clone(), child.clone()])
        .with_mask("sys-libs/zlib")
        .resolve([&atom])
        .await;
    let expected = Exhausted(
        atom,
        vec![RejectedCandidate::new(
            root,
            Dependency(
                Depend::VAR,
                Exhausted(
                    zlib_atom,
                    vec![RejectedCandidate::new(
                        child,
                        Policy(PolicyRejection::Masked(origin(1))),
                    )],
                )
                .into(),
            ),
        )],
    );

    assert_eq!(outcome.failures(), &[expected]);
}

#[tokio::test]
async fn test_resolve_strong_blocker() {
    let atom = atom("app-editors/nano");
    let root = pkg("app-editors/nano-2", &[("DEPEND", "!!net-misc/curl")]);
    let outcome = ResolverFixture::new([root.clone()]).resolve([&atom]).await;

    assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[Merge(EffectivePackage::new(root, EffectiveUse::default()))]
    );
}

#[tokio::test]
async fn test_resolve_any_of_rollback_blocker() {
    for prefix in ["!", "!!"] {
        let atom = atom("app-editors/nano");
        let depend = format!(
            "|| ( ( app-shells/bash {prefix}sys-libs/zlib dev-libs/libffi ) sys-libs/zlib )"
        );
        let root = pkg("app-editors/nano-2", &[("DEPEND", depend.as_str())]);
        let abandoned = pkg("app-shells/bash-2", &[]);
        let child = pkg("sys-libs/zlib-2", &[]);
        let abandoned_installed = installed("app-shells/bash-1", "0", &[], &[]);
        let child_installed = installed("sys-libs/zlib-2", "1", &[], &[]);
        let outcome = ResolverFixture::new([root.clone(), abandoned, child.clone()])
            .with_installed([abandoned_installed, child_installed])
            .resolve([&atom])
            .await;

        assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
        assert_eq!(
            outcome.plan().unwrap().operations(),
            &[
                Merge(EffectivePackage::new(child, EffectiveUse::default())),
                Merge(EffectivePackage::new(root, EffectiveUse::default()))
            ]
        );
    }
}

#[tokio::test]
async fn test_resolve_any_of_prefers_selected() {
    let atom = atom("app-editors/nano");
    let metadata = [(
        "DEPEND",
        "app-arch/bzip2 || ( app-arch/gzip app-arch/bzip2 )",
    )];
    let root = pkg("app-editors/nano-2", &metadata);
    let first = pkg("app-arch/gzip-2", &[]);
    let second = pkg("app-arch/bzip2-1", &[]);
    let outcome = ResolverFixture::new([root.clone(), first, second.clone()])
        .resolve([&atom])
        .await;

    assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[
            Merge(EffectivePackage::new(second, EffectiveUse::default())),
            Merge(EffectivePackage::new(root, EffectiveUse::default()))
        ]
    );
}

#[tokio::test]
async fn test_resolve_any_of_prefers_installed() {
    let atom = atom("app-editors/nano");
    let root = pkg(
        "app-editors/nano-2",
        &[("DEPEND", "|| ( app-arch/gzip app-arch/bzip2 )")],
    );
    let first = pkg("app-arch/gzip-2", &[]);
    let second = pkg("app-arch/bzip2-1", &[]);
    let installed = installed("app-arch/bzip2-1", "0", &[], &[]);
    let outcome = ResolverFixture::new([root.clone(), first, second.clone()])
        .with_installed([installed.clone()])
        .resolve([&atom])
        .await;

    assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[
            Replace(
                EffectivePackage::new(second, EffectiveUse::default()),
                installed
            ),
            Merge(EffectivePackage::new(root, EffectiveUse::default()))
        ]
    );
}

#[tokio::test]
async fn test_resolve_any_of_installed_over_selected() {
    let atom = atom("app-editors/nano");
    let root = pkg(
        "app-editors/nano-2",
        &[(
            "DEPEND",
            "app-arch/bzip2 || ( app-arch/gzip app-arch/bzip2 )",
        )],
    );
    let first = pkg("app-arch/gzip-2", &[]);
    let second = pkg("app-arch/bzip2-1", &[]);
    let installed = installed("app-arch/gzip-2", "0", &[], &[]);
    let outcome = ResolverFixture::new([root.clone(), first.clone(), second.clone()])
        .with_installed([installed.clone()])
        .resolve([&atom])
        .await;

    assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[
            Merge(EffectivePackage::new(second, EffectiveUse::default())),
            Replace(
                EffectivePackage::new(first, EffectiveUse::default()),
                installed
            ),
            Merge(EffectivePackage::new(root, EffectiveUse::default()))
        ]
    );
}

#[tokio::test]
async fn test_resolve_any_of_nested_group_rest() {
    let atom = atom("app-editors/nano");
    let root = pkg(
        "app-editors/nano-2",
        &[("DEPEND", "|| ( ( app-arch/bzip2 ) app-arch/gzip )")],
    );
    let nested = pkg("app-arch/bzip2-1", &[]);
    let plain = pkg("app-arch/gzip-2", &[]);
    let nested_installed = installed("app-arch/bzip2-1", "0", &[], &[]);
    let plain_installed = installed("app-arch/gzip-2", "0", &[], &[]);
    let outcome = ResolverFixture::new([root.clone(), nested, plain.clone()])
        .with_installed([nested_installed, plain_installed.clone()])
        .resolve([&atom])
        .await;

    assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[
            Replace(
                EffectivePackage::new(plain, EffectiveUse::default()),
                plain_installed
            ),
            Merge(EffectivePackage::new(root, EffectiveUse::default()))
        ]
    );
}

#[tokio::test]
async fn test_resolve_any_of_installed_use_mismatch() {
    let atom = atom("app-editors/nano");
    let root = pkg(
        "app-editors/nano-2",
        &[("DEPEND", "|| ( app-arch/bzip2 app-arch/gzip[foo] )")],
    );
    let first = pkg("app-arch/bzip2-1", &[]);
    let second = pkg("app-arch/gzip-2", &[("IUSE", "foo")]);
    let installed = installed("app-arch/gzip-2", "0", &["foo"], &[]);
    let outcome = ResolverFixture::new([root.clone(), first.clone(), second])
        .with_installed([installed])
        .with_use(&["foo"])
        .resolve([&atom])
        .await;

    assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[
            Merge(EffectivePackage::new(first, EffectiveUse::default())),
            Merge(EffectivePackage::new(root, EffectiveUse::default()))
        ]
    );
}

#[tokio::test]
async fn test_resolve_any_of_blocker_rest() {
    let atom = atom("app-editors/nano");
    let root = pkg(
        "app-editors/nano-2",
        &[("DEPEND", "|| ( dev-libs/libffi !sys-libs/zlib )")],
    );
    let libffi = pkg("dev-libs/libffi-1", &[]);
    let installed = installed("sys-libs/zlib-1", "0", &[], &[]);
    let outcome = ResolverFixture::new([root.clone(), libffi.clone()])
        .with_installed([installed])
        .resolve([&atom])
        .await;

    assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[
            Merge(EffectivePackage::new(libffi, EffectiveUse::default())),
            Merge(EffectivePackage::new(root, EffectiveUse::default()))
        ]
    );
}

#[tokio::test]
async fn test_resolve_any_of_use_conditional() {
    let atom = atom("app-editors/nano");
    let root = pkg(
        "app-editors/nano-2",
        &[
            ("IUSE", "foo"),
            ("DEPEND", "|| ( foo? ( sys-libs/zlib ) dev-libs/libffi )"),
        ],
    );
    let zlib = pkg("sys-libs/zlib-2", &[]);
    let libffi = pkg("dev-libs/libffi-1", &[]);
    let cases: [(&[&str], &Package); 2] = [(&["foo"], &zlib), (&[], &libffi)];

    for (enabled, expected) in cases {
        let outcome = ResolverFixture::new([root.clone(), zlib.clone(), libffi.clone()])
            .with_use(enabled)
            .resolve([&atom])
            .await;
        assert!(
            matches!(outcome, ResolutionOutcome::Complete { .. }),
            "{enabled:?}"
        );
        assert_eq!(
            outcome.plan().unwrap().operations(),
            &[
                Merge(EffectivePackage::new(
                    expected.clone(),
                    EffectiveUse::default()
                )),
                Merge(EffectivePackage::new(
                    root.clone(),
                    effective(&["foo"], enabled)
                ))
            ],
            "{enabled:?}"
        );
    }
}

#[tokio::test]
async fn test_resolve_any_of_empty() {
    let atom = atom("app-editors/nano");
    let root = pkg(
        "app-editors/nano-2",
        &[("IUSE", "foo"), ("DEPEND", "|| ( foo? ( sys-libs/zlib ) )")],
    );
    let zlib = pkg("sys-libs/zlib-2", &[]);
    let outcome = ResolverFixture::new([root.clone(), zlib])
        .resolve([&atom])
        .await;
    let expected = Exhausted(
        atom,
        vec![RejectedCandidate::new(
            root,
            Dependency(Depend::VAR, AnyOf(Vec::new()).into()),
        )],
    );

    assert_eq!(outcome.failures(), &[expected]);
}

#[tokio::test]
async fn test_resolve_any_of_blocker_installed() {
    let atom = atom("app-editors/nano");
    let root = pkg(
        "app-editors/nano-2",
        &[("DEPEND", "|| ( !sys-libs/zlib dev-libs/libffi )")],
    );
    let libffi = pkg("dev-libs/libffi-1", &[]);
    let installed = installed("sys-libs/zlib-1", "0", &[], &[]);
    let outcome = ResolverFixture::new([root.clone(), libffi.clone()])
        .with_installed([installed])
        .resolve([&atom])
        .await;

    assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[
            Merge(EffectivePackage::new(libffi, EffectiveUse::default())),
            Merge(EffectivePackage::new(root, EffectiveUse::default()))
        ]
    );
}

#[tokio::test]
async fn test_resolve_any_of_blocker_absent() {
    let atom = atom("app-editors/nano");
    let root = pkg(
        "app-editors/nano-2",
        &[("DEPEND", "|| ( !sys-libs/zlib dev-libs/libffi )")],
    );
    let libffi = pkg("dev-libs/libffi-1", &[]);
    let outcome = ResolverFixture::new([root.clone(), libffi])
        .resolve([&atom])
        .await;

    assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[Merge(EffectivePackage::new(root, EffectiveUse::default()))]
    );
}

#[tokio::test]
async fn test_resolve_any_of_blocker_active() {
    let root_atom = atom("app-editors/nano");
    let root = pkg(
        "app-editors/nano-2",
        &[(
            "DEPEND",
            "|| ( !sys-libs/zlib dev-libs/libffi ) sys-libs/zlib",
        )],
    );
    let zlib_atom = atom("sys-libs/zlib");
    let zlib = pkg("sys-libs/zlib-1", &[]);
    let outcome = ResolverFixture::new([root.clone(), zlib.clone()])
        .resolve([&root_atom])
        .await;
    let expected = Exhausted(
        root_atom,
        vec![RejectedCandidate::new(
            root.clone(),
            Dependency(
                Depend::VAR,
                Exhausted(
                    zlib_atom.clone(),
                    vec![RejectedCandidate::new(
                        zlib,
                        ActiveBlocker {
                            atom: zlib_atom,
                            owner: root.into(),
                        },
                    )],
                )
                .into(),
            ),
        )],
    );

    assert_eq!(outcome.failures(), &[expected]);
}

#[tokio::test]
async fn test_resolve_any_of_failures() {
    let first = atom("app-arch/gzip");
    let second = atom("app-arch/bzip2");
    let atom = atom("app-editors/nano");
    let metadata = [("DEPEND", "|| ( app-arch/gzip app-arch/bzip2 )")];
    let root = pkg("app-editors/nano-2", &metadata);
    let outcome = ResolverFixture::new([root.clone()]).resolve([&atom]).await;
    let expected = Exhausted(
        atom,
        vec![RejectedCandidate::new(
            root,
            Dependency(
                Depend::VAR,
                AnyOf(vec![NoCandidate(first), NoCandidate(second)]).into(),
            ),
        )],
    );

    assert_eq!(outcome.failures(), &[expected]);
}

#[tokio::test]
async fn test_resolve_multiple_roots() {
    let grep = atom("sys-apps/grep");
    let sed = atom("sys-apps/sed");
    let py313 = atom("dev-lang/python:3.13");
    let py314 = atom("dev-lang/python:3.14");
    let grep_pkg = pkg("sys-apps/grep-1", &[]);
    let sed_pkg = pkg("sys-apps/sed-1", &[]);
    let py313_pkg = pkg("dev-lang/python-3.13.5", &[("SLOT", "3.13")]);
    let py314_pkg = pkg("dev-lang/python-3.14.2", &[("SLOT", "3.14")]);
    let outcome = ResolverFixture::new([
        grep_pkg.clone(),
        sed_pkg.clone(),
        py313_pkg.clone(),
        py314_pkg.clone(),
    ])
    .resolve([&grep, &sed, &py313, &py314])
    .await;

    assert!(matches!(outcome, ResolutionOutcome::Complete { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[
            Merge(EffectivePackage::new(grep_pkg, EffectiveUse::default())),
            Merge(EffectivePackage::new(sed_pkg, EffectiveUse::default())),
            Merge(EffectivePackage::new(py313_pkg, EffectiveUse::default())),
            Merge(EffectivePackage::new(py314_pkg, EffectiveUse::default())),
        ]
    );
}

#[tokio::test]
async fn test_resolve_partial_roots() {
    let grep = atom("sys-apps/grep");
    let sed = atom("sys-apps/sed");
    let grep_pkg = pkg("sys-apps/grep-1", &[]);
    let outcome = ResolverFixture::new([grep_pkg.clone()])
        .resolve([&grep, &sed])
        .await;

    assert!(matches!(outcome, ResolutionOutcome::Partial { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[Merge(EffectivePackage::new(
            grep_pkg,
            EffectiveUse::default()
        ))]
    );
    assert_eq!(outcome.failures(), &[NoCandidate(sed)]);
}

#[tokio::test]
async fn test_resolve_failed_roots() {
    let grep = atom("sys-apps/grep");
    let sed = atom("sys-apps/sed");
    let outcome = ResolverFixture::new([]).resolve([&grep, &sed]).await;

    assert!(matches!(outcome, ResolutionOutcome::Failed { .. }));
    assert!(outcome.plan().is_none());
    assert_eq!(outcome.failures(), &[NoCandidate(grep), NoCandidate(sed)]);
}

#[tokio::test]
async fn test_resolve_partial_rollback() {
    let grep = atom("sys-apps/grep");
    let sed = atom("sys-apps/sed");
    let grep_pkg = pkg("sys-apps/grep-1", &[]);
    let sed_pkg = pkg(
        "sys-apps/sed-1",
        &[("DEPEND", "sys-libs/zlib dev-libs/libffi")],
    );
    let child = pkg("sys-libs/zlib-2", &[]);
    let outcome = ResolverFixture::new([grep_pkg.clone(), sed_pkg, child])
        .resolve([&grep, &sed])
        .await;

    assert!(matches!(outcome, ResolutionOutcome::Partial { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[Merge(EffectivePackage::new(
            grep_pkg,
            EffectiveUse::default()
        ))]
    );
}

#[tokio::test]
async fn test_resolve_partial_shared_selection() {
    let grep = atom("sys-apps/grep");
    let sed = atom("sys-apps/sed");
    let grep_pkg = pkg("sys-apps/grep-1", &[("DEPEND", "sys-libs/zlib")]);
    let sed_pkg = pkg(
        "sys-apps/sed-1",
        &[("DEPEND", "sys-libs/zlib dev-libs/libffi")],
    );
    let child = pkg("sys-libs/zlib-2", &[]);
    let outcome = ResolverFixture::new([grep_pkg.clone(), sed_pkg, child.clone()])
        .resolve([&grep, &sed])
        .await;

    assert!(matches!(outcome, ResolutionOutcome::Partial { .. }));
    assert_eq!(
        outcome.plan().unwrap().operations(),
        &[
            Merge(EffectivePackage::new(child, EffectiveUse::default())),
            Merge(EffectivePackage::new(grep_pkg, EffectiveUse::default()))
        ]
    );
}
