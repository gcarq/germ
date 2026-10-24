use std::cmp::Ordering;

use germ_pms::{Atom, BlockerStrength, Cpv, Package, PackageView, RepoName};
use log::info;

use super::outcome::{RequirementFailure, ResolutionOutcome};
use super::requirement::AtomRequirement;
use super::{EffectivePackage, ExecutionPlan, PackageOperation};
use crate::policy::PolicyRejection;
use crate::types::{FxIndexMap, FxIndexSet};
use crate::vdb::package::InstalledPackage;

/// Holds the current resolver state.
///
/// Every package blocker must hold in the final state.
#[derive(Debug, Default)]
pub struct ResolverState {
    /// Holds packages that have been selected for installation.
    selected: FxIndexMap<PackageId, EffectivePackage>,
    /// Holds temporary packages that are currently being visited.
    visiting: FxIndexMap<PackageId, EffectivePackage>,
    /// Holds rejected candidates.
    rejected: FxIndexMap<PackageId, PolicyRejection>,
    /// Holds active blockers that need to be resolved.
    blockers: Vec<ActiveBlocker>,
    /// Holds installed packages that are planned for removal after the owner is merged.
    removals: FxIndexSet<InstalledPackage>,
    /// Holds installed package versions that should be replaced.
    replacements: FxIndexMap<PackageId, InstalledPackage>,
}

impl ResolverState {
    /// Returns whether a selected or visiting package with `id` satisfies `requirement`.
    pub fn satisfies(
        &self,
        id: &PackageId,
        requirement: &AtomRequirement<'_>,
    ) -> anyhow::Result<bool> {
        self.selected
            .get(id)
            .or_else(|| self.visiting.get(id))
            .map_or(Ok(false), |pkg| pkg.matches(requirement))
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
    pub fn select(&mut self, id: PackageId) {
        let pkg = self
            .visiting
            .shift_remove(&id)
            .expect("BUG: selected package must be visited");
        self.selected.insert(id, pkg);
    }

    /// Checks if a package with the given `id` is already selected.
    pub fn is_selected(&self, id: &PackageId) -> bool {
        self.selected.contains_key(id)
    }

    /// Checks if a package with the given `id` is currently being visited.
    pub fn is_visiting(&self, id: &PackageId) -> bool {
        self.visiting.contains_key(id)
    }

    /// Marks a package as being visited.
    pub fn visit(&mut self, id: PackageId, candidate: &EffectivePackage) {
        self.visiting.insert(id, candidate.clone());
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
    /// atom. A weak blocker excludes its own package and is resolved by removing each
    /// matching installed package; a strong blocker in the same slot is handled automatically,
    /// in any other case it needs to be resolved manually.
    pub fn register_blocker(
        &mut self,
        owner: &EffectivePackage,
        atom: &Atom,
        strength: BlockerStrength,
        installed: impl IntoIterator<Item = InstalledPackage>,
    ) -> anyhow::Result<Option<Package>> {
        let requirement = AtomRequirement::dependency(atom, &owner.effective_use);
        for candidate in self.candidates() {
            if strength == BlockerStrength::Weak && candidate.pkg == owner.pkg {
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
            match strength {
                BlockerStrength::Weak => {
                    info!(
                        "{}: planning removal of {pkg} for blocker !{atom}",
                        owner.pkg
                    );
                    self.removals.insert(pkg);
                }
                BlockerStrength::Strong => return Ok(Some(pkg.into())),
            }
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
            selected: self.selected.len(),
            visiting: self.visiting.len(),
            blockers: self.blockers.len(),
            removals: self.removals.len(),
            replacements: self.replacements.len(),
        }
    }

    /// Rolls back to the given `checkpoint`.
    pub fn rollback(&mut self, checkpoint: StateCheckpoint) {
        self.selected.truncate(checkpoint.selected);
        self.visiting.truncate(checkpoint.visiting);
        self.blockers.truncate(checkpoint.blockers);
        self.removals.truncate(checkpoint.removals);
        self.replacements.truncate(checkpoint.replacements);
    }

    /// Records `installed` as replaced by the selected package with `id`.
    pub fn insert_replacement(&mut self, id: PackageId, installed: InstalledPackage) {
        self.replacements.insert(id, installed);
    }

    /// Consumes the state and creates a [`ResolutionOutcome`] for `roots` root atoms.
    ///
    /// The resolution is failed when every root failed, partial when only some failed,
    /// and complete when none failed.
    pub fn finalize(
        mut self,
        roots: usize,
        failures: Vec<RequirementFailure>,
    ) -> ResolutionOutcome {
        let mut ops = Vec::with_capacity(self.selected.len() + self.removals.len());
        for (id, sel) in self.selected {
            let op = match self.replacements.shift_remove(&id) {
                Some(installed) => match sel.pkg.cpv().cmp(installed.cpv()) {
                    Ordering::Equal => PackageOperation::Replace(sel, installed),
                    Ordering::Greater => PackageOperation::Upgrade(sel, installed),
                    Ordering::Less => PackageOperation::Downgrade(sel, installed),
                },
                None => PackageOperation::Merge(sel),
            };
            ops.push(op);
        }
        ops.extend(self.removals.into_iter().map(PackageOperation::Unmerge));
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
        self.selected.values().chain(self.visiting.values())
    }
}

/// A resolver state checkpoint for speculative traversal.
#[derive(Clone, Copy, Debug)]
pub struct StateCheckpoint {
    selected: usize,
    visiting: usize,
    blockers: usize,
    removals: usize,
    replacements: usize,
}

/// Represents a unique identifier for a package based on its CPV and repo name.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PackageId((Cpv, RepoName));

impl PackageId {
    pub fn new<P: PackageView>(pkg: &P) -> Self {
        Self((pkg.cpv().clone(), pkg.repo().clone()))
    }
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
    use crate::resolver::test_support::installed;
    use crate::useflag::EffectiveUse;

    fn selected_state(package: Package, replacement: Option<InstalledPackage>) -> ResolverState {
        let id = PackageId::new(&package);
        let mut state = ResolverState::default();
        state.visit(
            id.clone(),
            &EffectivePackage::new(package, EffectiveUse::default()),
        );
        state.select(id.clone());
        if let Some(installed) = replacement {
            state.insert_replacement(id, installed);
        }
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
    fn test_rollback_restores_state() {
        let pam = pkg("sys-libs/pam-1", &[]);
        let zlib = pkg("sys-libs/zlib-1", &[]);
        let glibc = pkg("sys-libs/glibc-1", &[]);
        let pam_id = PackageId::new(&pam);
        let glibc_id = PackageId::new(&glibc);
        let installed = installed("sys-libs/pam-2", "0", &[], &[]);
        let effective = EffectivePackage::new(pam, EffectiveUse::default());
        let mut state = ResolverState {
            selected: [(pam_id.clone(), effective.clone())].into_iter().collect(),
            visiting: [(
                PackageId::new(&zlib),
                EffectivePackage::new(zlib, EffectiveUse::default()),
            )]
            .into_iter()
            .collect(),
            rejected: [(glibc_id.clone(), PolicyRejection::Masked)]
                .into_iter()
                .collect(),
            blockers: vec![ActiveBlocker {
                owner: effective,
                atom: atom("sys-libs/db"),
            }],
            removals: [installed.clone()].into_iter().collect(),
            replacements: [(pam_id, installed)].into_iter().collect(),
        };
        let checkpoint = ResolverState::default().checkpoint();

        state.rollback(checkpoint);

        assert!(state.selected.is_empty());
        assert!(state.visiting.is_empty());
        assert!(state.blockers.is_empty());
        assert!(state.removals.is_empty());
        assert!(state.replacements.is_empty());
        assert_eq!(state.rejection(&glibc_id), Some(&PolicyRejection::Masked));
    }

    #[test]
    fn test_candidates() {
        let pam = pkg("sys-libs/pam-1", &[]);
        let zlib = pkg("sys-libs/zlib-1", &[]);
        let mut state = selected_state(pam.clone(), None);
        state.visit(
            PackageId::new(&zlib),
            &EffectivePackage::new(zlib.clone(), EffectiveUse::default()),
        );

        let candidates: Vec<_> = state.candidates().map(|candidate| &candidate.pkg).collect();
        assert_eq!(candidates, vec![&pam, &zlib]);
    }
}
