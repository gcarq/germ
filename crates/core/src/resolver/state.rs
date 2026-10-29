use std::cmp::Ordering;

use germ_pms::{Atom, BlockerStrength, Cpv, Package, PackageView, RepoName};
use log::debug;

use super::outcome::{RequirementFailure, ResolutionOutcome};
use super::requirement::AtomRequirement;
use super::{EffectivePackage, ExecutionPlan, PackageOperation};
use crate::policy::PolicyRejection;
use crate::types::{FxIndexMap, FxIndexSet};
use crate::vdb::package::InstalledPackage;

/// Represents the state of a candidate package in the resolver.
#[derive(Debug)]
enum CandidateState {
    /// Package that is currently being visited.
    Visiting(EffectivePackage),
    /// Package that has been selected for the execution plan.
    Selected {
        package: EffectivePackage,
        replaces: Option<Box<InstalledPackage>>,
    },
}

impl CandidateState {
    const fn package(&self) -> &EffectivePackage {
        match self {
            Self::Visiting(package) | Self::Selected { package, .. } => package,
        }
    }

    fn into_operation(self) -> Option<PackageOperation> {
        let Self::Selected { package, replaces } = self else {
            return None;
        };
        Some(match replaces {
            Some(installed) => match package.pkg.cpv().cmp(installed.cpv()) {
                Ordering::Equal => PackageOperation::Replace(package, *installed),
                Ordering::Greater => PackageOperation::Upgrade(package, *installed),
                Ordering::Less => PackageOperation::Downgrade(package, *installed),
            },
            None => PackageOperation::Merge(package),
        })
    }
}

/// Holds the current resolver state.
///
/// Every package blocker must hold in the final state.
#[derive(Debug, Default)]
pub struct ResolverState {
    /// Holds candidate records, with selected candidates in commit order.
    candidates: FxIndexMap<PackageId, CandidateState>,
    /// Holds rejected candidates.
    rejected: FxIndexMap<PackageId, PolicyRejection>,
    /// Holds active blockers that need to be resolved.
    blockers: Vec<ActiveBlocker>,
    /// Holds installed packages that are planned for removal after the owner is merged.
    removals: FxIndexSet<InstalledPackage>,
}

impl ResolverState {
    /// Returns the candidate package for the given `id` or `None` if it is not known yet.
    pub fn candidate(&self, id: &PackageId) -> Option<&EffectivePackage> {
        self.candidates.get(id).map(CandidateState::package)
    }

    /// Returns whether a selected or visiting package satisfies `requirement`.
    pub fn already_satisfied(&self, requirement: &AtomRequirement<'_>) -> anyhow::Result<bool> {
        for candidate in self.candidates() {
            if candidate.matches(requirement)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Returns a selected candidate that has the same primary slot as `pkg`.
    ///
    /// A same slot conflict only happens if the package version differs.
    pub fn slot_conflict(&self, pkg: &Package) -> Option<&EffectivePackage> {
        self.candidates()
            .find(|selected| selected.pkg.matches_package_slot(pkg) && selected.pkg != *pkg)
    }

    /// Marks the package for the given `id` as selected.
    ///
    /// `replaces` is the installed package version the selected package supersedes in its slot.
    pub fn select(&mut self, id: PackageId, replaces: Option<InstalledPackage>) {
        // We have to use `shift_remove()` and `insert()` to preserve the order,
        // because `StateCheckpoint` just truncates `candidates` to a previous length.
        let Some(CandidateState::Visiting(package)) = self.candidates.shift_remove(&id) else {
            panic!("BUG: selected package must be visited");
        };
        self.candidates.insert(
            id,
            CandidateState::Selected {
                package,
                replaces: replaces.map(Box::new),
            },
        );
    }

    /// Marks a package as being visited.
    pub fn visit(&mut self, id: PackageId, candidate: &EffectivePackage) {
        self.candidates
            .insert(id, CandidateState::Visiting(candidate.clone()));
    }

    /// Marks a package as rejected.
    pub fn reject(&mut self, id: PackageId, reason: PolicyRejection) {
        if !self.rejected.contains_key(&id) {
            self.rejected.insert(id, reason);
        }
    }

    /// Returns the candidate rejection for `id`.
    pub fn rejection(&self, id: &PackageId) -> Option<&PolicyRejection> {
        self.rejected.get(id)
    }

    /// Registers a package blocker, or returns a package that conflicts with it.
    ///
    /// A blocker conflicts with a selected, visiting, or installed package that matches its
    /// atom.
    ///
    /// In [`BlockerMode::Requirement`], a weak blocker excludes its own package and plans the
    /// removal of the installed package. A strong blocker is enforced.
    ///
    /// In [`BlockerMode::AnyOfMember`], a installed package conflicts and plans no
    /// removal. A blocker without a conflict is recorded as an active blocker.
    pub fn register_blocker(
        &mut self,
        owner: &EffectivePackage,
        atom: &Atom,
        strength: BlockerStrength,
        mode: BlockerMode,
        installed: impl IntoIterator<Item = InstalledPackage>,
    ) -> anyhow::Result<Option<Package>> {
        let requirement = AtomRequirement::dependency(atom, &owner.effective_use);
        for candidate in self.candidates() {
            if strength.is_weak() && candidate.pkg == owner.pkg {
                continue;
            }
            if candidate.matches(&requirement)? {
                return Ok(Some(candidate.pkg.clone()));
            }
        }
        for pkg in installed {
            if pkg.matches_package_slot(&owner.pkg) {
                continue;
            }
            if !requirement.satisfied_by(&pkg, pkg.effective_use())? {
                continue;
            }
            if strength.is_weak() && mode == BlockerMode::Requirement {
                debug!(
                    "{}: planning removal of {pkg} for blocker !{atom}",
                    owner.pkg
                );
                self.removals.insert(pkg);
                continue;
            }
            return Ok(Some(pkg.into()));
        }
        self.blockers.push(ActiveBlocker::new(owner, atom));
        Ok(None)
    }

    /// Returns the first active blocker that matches `candidate`.
    pub fn matching_active_blocker(
        &self,
        candidate: &EffectivePackage,
    ) -> anyhow::Result<Option<(&Atom, &Package)>> {
        for blocker in &self.blockers {
            if blocker.matches(candidate)? {
                return Ok(Some((&blocker.atom, &blocker.owner.pkg)));
            }
        }
        Ok(None)
    }

    /// Creates a [`StateCheckpoint`] for speculative resolver state.
    pub fn checkpoint(&self) -> StateCheckpoint {
        StateCheckpoint {
            candidates: self.candidates.len(),
            blockers: self.blockers.len(),
            removals: self.removals.len(),
        }
    }

    /// Rolls back to the given `checkpoint`.
    pub fn rollback(&mut self, checkpoint: StateCheckpoint) {
        self.candidates.truncate(checkpoint.candidates);
        self.blockers.truncate(checkpoint.blockers);
        self.removals.truncate(checkpoint.removals);
    }

    /// Consumes the state and creates a [`ResolutionOutcome`] for `roots` root atoms.
    ///
    /// The resolution is failed when every root failed, partial when only some failed,
    /// and complete when none failed.
    pub fn finalize(self, roots: usize, failures: Vec<RequirementFailure>) -> ResolutionOutcome {
        let ops = self
            .candidates
            .into_values()
            .filter_map(CandidateState::into_operation)
            .chain(self.removals.into_iter().map(PackageOperation::Unmerge))
            .collect();
        let plan = ExecutionPlan::new(ops);
        let rejected = self.rejected;
        if failures.is_empty() {
            ResolutionOutcome::Complete { plan, rejected }
        } else if failures.len() == roots {
            ResolutionOutcome::Failed { failures, rejected }
        } else {
            ResolutionOutcome::Partial {
                plan,
                failures,
                rejected,
            }
        }
    }

    /// Returns an iterator over all selected and "visiting" candidates.
    fn candidates(&self) -> impl Iterator<Item = &EffectivePackage> {
        self.candidates.values().map(CandidateState::package)
    }
}

/// A resolver state checkpoint for speculative traversal.
#[derive(Clone, Copy, Debug)]
pub struct StateCheckpoint {
    candidates: usize,
    blockers: usize,
    removals: usize,
}

/// Represents a unique identifier for a package based on its CPV and repo name.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PackageId((Cpv, RepoName));

impl PackageId {
    pub fn new<P: PackageView>(pkg: &P) -> Self {
        Self((pkg.cpv().clone(), pkg.repo().clone()))
    }
}

/// Defines how a package blocker is handled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockerMode {
    /// A weak blocker plans removal of matching installed packages.
    Requirement,
    /// A matching installed package makes the blocker unmatched.
    AnyOfMember,
}

/// Defines an active package blocker in a dependency expression.
#[derive(Clone, Debug)]
struct ActiveBlocker {
    owner: EffectivePackage,
    atom: Atom,
}

impl ActiveBlocker {
    fn new(owner: &EffectivePackage, atom: &Atom) -> Self {
        Self {
            owner: owner.clone(),
            atom: atom.clone(),
        }
    }

    fn matches(&self, candidate: &EffectivePackage) -> anyhow::Result<bool> {
        AtomRequirement::dependency(&self.atom, &self.owner.effective_use)
            .satisfied_by(&candidate.pkg, &candidate.effective_use)
    }
}

#[cfg(test)]
mod tests {
    use germ_pms::test_support::{atom, pkg};

    use super::*;
    use crate::files::test_support::origin;
    use crate::resolver::test_support::installed;
    use crate::useflag::EffectiveUse;

    fn selected_state(package: Package, replacement: Option<InstalledPackage>) -> ResolverState {
        let id = PackageId::new(&package);
        let mut state = ResolverState::default();
        state.visit(
            id.clone(),
            &EffectivePackage::new(package, EffectiveUse::default()),
        );
        state.select(id, replacement);
        state
    }

    #[test]
    fn test_finalize_operations() {
        #[derive(Clone, Copy)]
        enum Kind {
            Merge,
            Replace,
            Upgrade,
            Downgrade,
        }

        let cases = [
            ("sys-libs/db-2", None, Kind::Merge),
            ("sys-libs/db-2", Some("sys-libs/db-2"), Kind::Replace),
            ("sys-libs/db-2", Some("sys-libs/db-1"), Kind::Upgrade),
            ("sys-libs/db-1", Some("sys-libs/db-2"), Kind::Downgrade),
        ];

        for (fqn, previous, kind) in cases {
            let package = pkg(fqn, &[]);
            let previous = previous.map(|fqn| installed(fqn, "0", &[], &[]));
            let outcome = selected_state(package.clone(), previous.clone()).finalize(1, Vec::new());
            let effective = EffectivePackage::new(package, EffectiveUse::default());
            let expected = match kind {
                Kind::Merge => PackageOperation::Merge(effective),
                Kind::Replace => PackageOperation::Replace(effective, previous.unwrap()),
                Kind::Upgrade => PackageOperation::Upgrade(effective, previous.unwrap()),
                Kind::Downgrade => PackageOperation::Downgrade(effective, previous.unwrap()),
            };
            assert_eq!(outcome.plan().unwrap().operations(), &[expected], "{fqn}");
        }
    }

    #[test]
    fn test_rollback() -> anyhow::Result<()> {
        let pam = pkg("sys-libs/pam-1", &[]);
        let zlib = pkg("sys-libs/zlib-1", &[]);
        let glibc = pkg("sys-libs/glibc-1", &[]);
        let replaced = installed("sys-libs/pam-1", "0", &[], &[]);
        let removal = installed("sys-libs/db-1", "0", &[], &[]);
        let effective = EffectivePackage::new(pam.clone(), EffectiveUse::default());
        let mut state = selected_state(pam, Some(replaced.clone()));
        let glibc_id = PackageId::new(&glibc);
        let zlib_id = PackageId::new(&zlib);
        state.reject(glibc_id.clone(), PolicyRejection::Masked(origin(1)));
        let checkpoint = state.checkpoint();

        state.visit(
            zlib_id.clone(),
            &EffectivePackage::new(zlib, EffectiveUse::default()),
        );
        state.visit(
            glibc_id.clone(),
            &EffectivePackage::new(glibc, EffectiveUse::default()),
        );
        state.select(zlib_id, None);
        assert!(
            state
                .register_blocker(
                    &effective,
                    &atom("sys-libs/db"),
                    BlockerStrength::Weak,
                    BlockerMode::Requirement,
                    [removal],
                )?
                .is_none()
        );

        state.rollback(checkpoint);

        assert!(state.blockers.is_empty());
        let outcome = state.finalize(1, Vec::new());
        assert_eq!(
            outcome.plan().unwrap().operations(),
            &[PackageOperation::Replace(effective, replaced)]
        );
        assert_eq!(
            outcome.rejected().get(&glibc_id).cloned(),
            Some(PolicyRejection::Masked(origin(1)))
        );
        Ok(())
    }
}
