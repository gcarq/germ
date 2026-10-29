use germ_pms::BlockerStrength;
use germ_pms::test_support::{atom, pkg};

use super::CandidateRejectionReason::{ActiveBlocker, SlotConflict};
use super::RequirementFailure::{AnyOf, Blocker, Exhausted, NoCandidate};
use super::test_support::{
    ResolverFixture, assert_complete_plan, assert_partial_plan, dependency_rejection,
    exhausted_root, installed, merge, merge_with, replace, unmerge, upgrade,
};
use super::*;
use crate::files::test_support::origin;
use crate::policy::PolicyRejection;
use crate::useflag::test_support::effective;

#[tokio::test]
async fn test_resolve_use_conditional() {
    let zlib_atom = atom("sys-libs/zlib");
    let atom = atom("app-editors/nano");
    let root = pkg(
        "app-editors/nano-2",
        &[
            ("IUSE", "feature"),
            ("DEPEND", "feature? ( sys-libs/zlib )"),
        ],
    );
    let child = pkg("sys-libs/zlib-2", &[]);

    let enabled = ResolverFixture::new([&root, &child])
        .with_use(&["feature"])
        .resolve([&atom])
        .await;
    assert_complete_plan(
        &enabled,
        &[
            merge(child),
            merge_with(root.clone(), effective(&["feature"], &["feature"])),
        ],
    );

    let disabled = ResolverFixture::new([&root]).resolve([&atom]).await;
    assert_complete_plan(
        &disabled,
        &[merge_with(root.clone(), effective(&["feature"], &[]))],
    );

    let unavailable = ResolverFixture::new([&root])
        .with_use(&["feature"])
        .resolve([&atom])
        .await;
    let expected = exhausted_root(atom, root, NoCandidate(zlib_atom));
    assert_eq!(unavailable.failures(), &[expected]);
}

#[tokio::test]
async fn test_resolve_fallback_masked() {
    let atom = atom("app-editors/nano");
    let root = pkg("app-editors/nano-2", &[("DEPEND", "sys-libs/zlib")]);
    let zlib_2 = pkg("sys-libs/zlib-2", &[]);
    let zlib_1 = pkg("sys-libs/zlib-1", &[]);

    let outcome = ResolverFixture::new([&root, &zlib_2, &zlib_1])
        .with_mask("=sys-libs/zlib-2")
        .resolve([&atom])
        .await;
    assert_complete_plan(&outcome, &[merge(zlib_1), merge(root)]);
}

#[tokio::test]
async fn test_resolve_selected() {
    let atom = atom("app-editors/nano");
    let zlib_2 = pkg("sys-libs/zlib-2", &[]);
    let zlib_1 = pkg("sys-libs/zlib-1", &[]);
    let cases = [
        ("sys-libs/zlib sys-libs/zlib", &zlib_2),
        ("=sys-libs/zlib-1 sys-libs/zlib", &zlib_1),
    ];

    for (depend, selected) in cases {
        let root = pkg("app-editors/nano-2", &[("DEPEND", depend)]);
        let outcome = ResolverFixture::new([&root, &zlib_2, &zlib_1])
            .resolve([&atom])
            .await;

        assert_complete_plan(&outcome, &[merge(selected.clone()), merge(root)]);
    }
}

#[tokio::test]
async fn test_resolve_dependency_cycle() {
    let atom = atom("app-editors/nano");
    let root = pkg("app-editors/nano-2", &[("DEPEND", "sys-libs/zlib")]);
    let child = pkg("sys-libs/zlib-2", &[("DEPEND", "app-editors/nano")]);
    let outcome = ResolverFixture::new([&root, &child]).resolve([&atom]).await;

    assert_complete_plan(&outcome, &[merge(child), merge(root)]);
}

#[tokio::test]
async fn test_resolve_slot_conflict() {
    let older_atom = atom("=app/lib-1");
    let atom = atom("app-editors/nano");
    for (newer_slot, older_slot) in [("0", "0"), ("0/2", "0/1")] {
        let root = pkg("app-editors/nano-2", &[("DEPEND", "app/lib =app/lib-1")]);
        let newer = pkg("app/lib-2", &[("SLOT", newer_slot)]);
        let older = pkg("app/lib-1", &[("SLOT", older_slot)]);
        let outcome = ResolverFixture::new([&root, &newer, &older])
            .resolve([&atom])
            .await;
        let expected = exhausted_root(
            atom.clone(),
            root,
            Exhausted(
                older_atom.clone(),
                vec![RejectedCandidate::new(
                    older,
                    SlotConflict {
                        selected: newer.into(),
                    },
                )],
            ),
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
    let outcome = ResolverFixture::new([&new_root, &old_root, &first, &second, &fallback])
        .with_installed([&first_installed, &fallback_installed])
        .resolve([&atom])
        .await;

    assert_complete_plan(&outcome, &[merge(fallback), merge(old_root)]);
}

#[tokio::test]
async fn test_plan_replace() {
    let atom = atom("app-editors/nano");
    let root = pkg("app-editors/nano-2", &[]);
    let installed = installed("app-editors/nano-2", "0", &[], &[]);
    let outcome = ResolverFixture::new([&root])
        .with_installed([&installed])
        .resolve([&atom])
        .await;

    assert_complete_plan(&outcome, &[replace(root, installed)]);
}

#[tokio::test]
async fn test_plan_other_slot_merge() {
    let atom = atom("app-editors/nano");
    let root = pkg("app-editors/nano-2", &[]);
    let installed = installed("app-editors/nano-2", "1", &[], &[]);
    let outcome = ResolverFixture::new([&root])
        .with_installed([&installed])
        .resolve([&atom])
        .await;

    assert_complete_plan(&outcome, &[merge(root)]);
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
        let outcome = ResolverFixture::new([&root, &owner, &blocked])
            .resolve([&atom])
            .await;
        let expected = exhausted_root(
            atom.clone(),
            root,
            Exhausted(
                blocked_atom.clone(),
                vec![RejectedCandidate::new(
                    blocked,
                    ActiveBlocker {
                        atom: blocked_atom.clone(),
                        owner: owner.into(),
                    },
                )],
            ),
        );

        assert_eq!(outcome.failures(), &[expected], "{prefix}");
    }
}

#[tokio::test]
async fn test_resolve_blocker_in_graph() {
    for (prefix, strength) in [
        ("!", BlockerStrength::Weak),
        ("!!", BlockerStrength::Strong),
    ] {
        let sed_atom = atom("sys-apps/sed");
        let nano_atom = atom("app-editors/nano");

        let depend = format!("{prefix}sys-apps/sed");
        let root = pkg(
            "app-editors/nano-2",
            &[("DEPEND", "sys-apps/sed sys-apps/grep")],
        );
        let owner = pkg("sys-apps/grep-1", &[("DEPEND", depend.as_str())]);
        let conflict = pkg("sys-apps/sed-1", &[]);
        let outcome = ResolverFixture::new([&root, &owner, &conflict])
            .resolve([&nano_atom])
            .await;
        let expected = exhausted_root(
            nano_atom.clone(),
            root,
            Exhausted(
                atom("sys-apps/grep"),
                vec![dependency_rejection(
                    owner,
                    Blocker {
                        strength,
                        atom: sed_atom,
                        conflict: conflict.into(),
                    },
                )],
            ),
        );

        assert_eq!(
            outcome.failures(),
            &[expected],
            "{prefix} blocker on selected"
        );

        let depend = format!("{prefix}app-editors/nano");
        let root = pkg("app-editors/nano-2", &[("DEPEND", "sys-libs/zlib")]);
        let child = pkg("sys-libs/zlib-2", &[("DEPEND", depend.as_str())]);
        let outcome = ResolverFixture::new([&root, &child])
            .resolve([&nano_atom])
            .await;
        let expected = exhausted_root(
            nano_atom.clone(),
            root.clone(),
            Exhausted(
                atom("sys-libs/zlib"),
                vec![dependency_rejection(
                    child,
                    Blocker {
                        strength,
                        atom: nano_atom.clone(),
                        conflict: root.into(),
                    },
                )],
            ),
        );

        assert_eq!(
            outcome.failures(),
            &[expected],
            "{prefix} blocker on visiting"
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
        let outcome = ResolverFixture::new([&root])
            .with_installed([&other])
            .resolve([&atom])
            .await;

        match strength {
            BlockerStrength::Weak => {
                assert_complete_plan(&outcome, &[merge(root), unmerge(other)]);
            }
            BlockerStrength::Strong => {
                let expected = exhausted_root(
                    atom,
                    root,
                    Blocker {
                        strength,
                        atom: pam_atom,
                        conflict: pkg("sys-libs/pam-2", &[]).into(),
                    },
                );
                assert_eq!(outcome.failures(), &[expected], "{prefix}");
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
        let outcome = ResolverFixture::new([&root])
            .with_installed([&installed])
            .resolve([&atom])
            .await;

        assert_complete_plan(&outcome, &[upgrade(root, installed)]);
    }
}

#[tokio::test]
async fn test_resolve_blocker_use() {
    let atom = atom("app-editors/nano");

    let root = pkg("app-editors/nano-2", &[("DEPEND", "!sys-libs/pam[flag]")]);
    let disabled = installed("sys-libs/pam-2", "0", &["flag"], &[]);
    let outcome = ResolverFixture::new([&root])
        .with_installed([&disabled])
        .resolve([&atom])
        .await;
    assert_complete_plan(&outcome, &[merge(root.clone())]);

    let enabled = installed("sys-libs/pam-2", "0", &["flag"], &["flag"]);
    let outcome = ResolverFixture::new([&root])
        .with_installed([&enabled])
        .resolve([&atom])
        .await;
    assert_complete_plan(&outcome, &[merge(root.clone()), unmerge(enabled)]);

    let root = pkg(
        "app-editors/nano-2",
        &[("IUSE", "feature"), ("DEPEND", "!sys-libs/pam[feature?]")],
    );
    let enabled = installed("sys-libs/pam-2", "0", &["feature"], &["feature"]);
    let disabled = installed("sys-libs/pam-1", "0", &["feature"], &[]);
    let outcome = ResolverFixture::new([&root])
        .with_use(&["feature"])
        .with_installed([&disabled, &enabled])
        .resolve([&atom])
        .await;
    let merged = merge_with(root, effective(&["feature"], &["feature"]));
    assert_complete_plan(&outcome, &[merged, unmerge(enabled)]);
}

#[tokio::test]
async fn test_resolve_missing_dependency_candidate() {
    let missing = atom("dev-libs/libffi");
    let atom = atom("app-editors/nano");
    let root = pkg("app-editors/nano-2", &[("DEPEND", "dev-libs/libffi")]);
    let outcome = ResolverFixture::new([&root]).resolve([&atom]).await;
    let expected = exhausted_root(atom, root, NoCandidate(missing));

    assert_eq!(outcome.failures(), &[expected]);
    assert!(outcome.rejected().is_empty());
}

#[tokio::test]
async fn test_resolve_rejected_dependency_candidates() {
    let zlib_atom = atom("sys-libs/zlib");
    let atom = atom("app-editors/nano");
    let root = pkg("app-editors/nano-2", &[("DEPEND", "sys-libs/zlib")]);
    let child = pkg("sys-libs/zlib-2", &[]);
    let outcome = ResolverFixture::new([&root, &child])
        .with_mask("sys-libs/zlib")
        .resolve([&atom])
        .await;
    let expected = exhausted_root(
        atom,
        root,
        Exhausted(
            zlib_atom,
            vec![RejectedCandidate::policy(
                child,
                PolicyRejection::Masked(origin(1)),
            )],
        ),
    );

    assert_eq!(outcome.failures(), &[expected]);
}

#[tokio::test]
async fn test_resolve_strong_blocker_absent() {
    let atom = atom("app-editors/nano");
    let root = pkg("app-editors/nano-2", &[("DEPEND", "!!net-misc/curl")]);
    let outcome = ResolverFixture::new([&root]).resolve([&atom]).await;

    assert_complete_plan(&outcome, &[merge(root)]);
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
        let outcome = ResolverFixture::new([&root, &abandoned, &child])
            .with_installed([&abandoned_installed, &child_installed])
            .resolve([&atom])
            .await;

        assert_complete_plan(&outcome, &[merge(child), merge(root)]);
    }
}

#[tokio::test]
async fn test_resolve_any_of_prefers_selected() {
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
    let outcome = ResolverFixture::new([&root, &first, &second])
        .resolve([&atom])
        .await;

    assert_complete_plan(&outcome, &[merge(second), merge(root)]);
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
    let outcome = ResolverFixture::new([&root, &first, &second])
        .with_installed([&installed])
        .resolve([&atom])
        .await;

    assert_complete_plan(&outcome, &[replace(second, installed), merge(root)]);
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
    let outcome = ResolverFixture::new([&root, &first, &second])
        .with_installed([&installed])
        .resolve([&atom])
        .await;

    assert_complete_plan(
        &outcome,
        &[merge(second), replace(first, installed), merge(root)],
    );
}

#[tokio::test]
async fn test_resolve_any_of_nested_group_remaining() {
    let atom = atom("app-editors/nano");
    let root = pkg(
        "app-editors/nano-2",
        &[("DEPEND", "|| ( ( app-arch/bzip2 ) app-arch/gzip )")],
    );
    let nested = pkg("app-arch/bzip2-1", &[]);
    let plain = pkg("app-arch/gzip-2", &[]);
    let nested_installed = installed("app-arch/bzip2-1", "0", &[], &[]);
    let plain_installed = installed("app-arch/gzip-2", "0", &[], &[]);
    let outcome = ResolverFixture::new([&root, &nested, &plain])
        .with_installed([&nested_installed, &plain_installed])
        .resolve([&atom])
        .await;

    assert_complete_plan(&outcome, &[replace(plain, plain_installed), merge(root)]);
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
    let outcome = ResolverFixture::new([&root, &first, &second])
        .with_installed([&installed])
        .with_use(&["foo"])
        .resolve([&atom])
        .await;

    assert_complete_plan(&outcome, &[merge(first), merge(root)]);
}

#[tokio::test]
async fn test_resolve_any_of_blocker_remaining() {
    let atom = atom("app-editors/nano");
    let root = pkg(
        "app-editors/nano-2",
        &[("DEPEND", "|| ( dev-libs/libffi !sys-libs/zlib )")],
    );
    let libffi = pkg("dev-libs/libffi-1", &[]);
    let installed = installed("sys-libs/zlib-1", "0", &[], &[]);
    let outcome = ResolverFixture::new([&root, &libffi])
        .with_installed([&installed])
        .resolve([&atom])
        .await;

    assert_complete_plan(&outcome, &[merge(libffi), merge(root)]);
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
        let outcome = ResolverFixture::new([&root, &zlib, &libffi])
            .with_use(enabled)
            .resolve([&atom])
            .await;
        let merged = merge_with(root.clone(), effective(&["foo"], enabled));
        assert_complete_plan(&outcome, &[merge(expected.clone()), merged]);
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
    let outcome = ResolverFixture::new([&root, &zlib]).resolve([&atom]).await;
    let expected = exhausted_root(atom, root, AnyOf(Vec::new()));

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
    let outcome = ResolverFixture::new([&root, &libffi])
        .with_installed([&installed])
        .resolve([&atom])
        .await;

    assert_complete_plan(&outcome, &[merge(libffi), merge(root)]);
}

#[tokio::test]
async fn test_resolve_any_of_blocker_absent() {
    let atom = atom("app-editors/nano");
    let root = pkg(
        "app-editors/nano-2",
        &[("DEPEND", "|| ( !sys-libs/zlib dev-libs/libffi )")],
    );
    let libffi = pkg("dev-libs/libffi-1", &[]);
    let outcome = ResolverFixture::new([&root, &libffi])
        .resolve([&atom])
        .await;

    assert_complete_plan(&outcome, &[merge(root)]);
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
    let outcome = ResolverFixture::new([&root, &zlib])
        .resolve([&root_atom])
        .await;
    let expected = exhausted_root(
        root_atom,
        root.clone(),
        Exhausted(
            zlib_atom.clone(),
            vec![RejectedCandidate::new(
                zlib,
                ActiveBlocker {
                    atom: zlib_atom,
                    owner: root.into(),
                },
            )],
        ),
    );

    assert_eq!(outcome.failures(), &[expected]);
}

#[tokio::test]
async fn test_resolve_any_of_failures() {
    let first = atom("app-arch/gzip");
    let second = atom("app-arch/bzip2");
    let atom = atom("app-editors/nano");
    let root = pkg(
        "app-editors/nano-2",
        &[("DEPEND", "|| ( app-arch/gzip app-arch/bzip2 )")],
    );
    let outcome = ResolverFixture::new([&root]).resolve([&atom]).await;
    let expected = exhausted_root(
        atom,
        root,
        AnyOf(vec![NoCandidate(first), NoCandidate(second)]),
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
    let outcome = ResolverFixture::new([&grep_pkg, &sed_pkg, &py313_pkg, &py314_pkg])
        .resolve([&grep, &sed, &py313, &py314])
        .await;

    assert_complete_plan(
        &outcome,
        &[
            merge(grep_pkg),
            merge(sed_pkg),
            merge(py313_pkg),
            merge(py314_pkg),
        ],
    );
}

#[tokio::test]
async fn test_resolve_partial_roots() {
    let grep = atom("sys-apps/grep");
    let sed = atom("sys-apps/sed");
    let grep_pkg = pkg("sys-apps/grep-1", &[]);
    let outcome = ResolverFixture::new([&grep_pkg])
        .resolve([&grep, &sed])
        .await;

    assert_partial_plan(&outcome, &[merge(grep_pkg)]);
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
    let outcome = ResolverFixture::new([&grep_pkg, &sed_pkg, &child])
        .resolve([&grep, &sed])
        .await;

    assert_partial_plan(&outcome, &[merge(grep_pkg)]);
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
    let outcome = ResolverFixture::new([&grep_pkg, &sed_pkg, &child])
        .resolve([&grep, &sed])
        .await;

    assert_partial_plan(&outcome, &[merge(child), merge(grep_pkg)]);
}
