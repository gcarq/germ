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
        for candidate in self.selected.values().chain(self.visiting.values()) {
            if candidate.matches(requirement)? {
                return Ok(true);
            }
        }
        Ok(false)
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
        for candidate in self.selected.values().chain(self.visiting.values()) {
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

    /// Consumes the state and creates a [`ResolutionOutcome`].
    pub fn finalize(mut self, failure: Option<RequirementFailure>) -> ResolutionOutcome {
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
        ResolutionOutcome::new(failure, ExecutionPlan::new(ops), self.rejected)
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
    use germ_pms::test_support::pkg;

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
    fn test_finalize_merge() {
        let pkg = pkg("app-misc", "foo", "1", &[]);
        let outcome = selected_state(pkg.clone(), None).finalize(None);

        assert_eq!(
            outcome.plan().operations(),
            &[PackageOperation::Merge(EffectivePackage::new(
                pkg,
                EffectiveUse::default(),
            ))]
        );
    }

    #[test]
    fn test_finalize_replace() {
        let pkg = pkg("app-misc", "foo", "1", &[]);
        let installed = installed("foo", "1", "0", &[], &[]);
        let outcome = selected_state(pkg.clone(), Some(installed.clone())).finalize(None);

        assert_eq!(
            outcome.plan().operations(),
            &[PackageOperation::Replace(
                EffectivePackage::new(pkg, EffectiveUse::default()),
                installed,
            )]
        );
    }

    #[test]
    fn test_finalize_upgrade() {
        let pkg = pkg("app-misc", "foo", "2", &[]);
        let installed = installed("foo", "1", "0", &[], &[]);
        let outcome = selected_state(pkg.clone(), Some(installed.clone())).finalize(None);

        assert_eq!(
            outcome.plan().operations(),
            &[PackageOperation::Upgrade(
                EffectivePackage::new(pkg, EffectiveUse::default()),
                installed,
            )]
        );
    }

    #[test]
    fn test_finalize_downgrade() {
        let pkg = pkg("app-misc", "foo", "1", &[]);
        let installed = installed("foo", "2", "0", &[], &[]);
        let outcome = selected_state(pkg.clone(), Some(installed.clone())).finalize(None);

        assert_eq!(
            outcome.plan().operations(),
            &[PackageOperation::Downgrade(
                EffectivePackage::new(pkg, EffectiveUse::default()),
                installed,
            )]
        );
    }

    #[test]
    fn test_rollback_restores_state() {
        let foo = pkg("app-misc", "foo", "1", &[]);
        let bar = pkg("app-misc", "bar", "1", &[]);
        let mask = pkg("app-misc", "mask", "1", &[]);
        let foo_id = PackageId::new(&foo);
        let mask_id = PackageId::new(&mask);
        let installed = installed("foo", "0", "0", &[], &[]);
        let effective = EffectivePackage::new(foo, EffectiveUse::default());
        let mut state = ResolverState {
            selected: [(foo_id.clone(), effective.clone())].into_iter().collect(),
            visiting: [(
                PackageId::new(&bar),
                EffectivePackage::new(bar, EffectiveUse::default()),
            )]
            .into_iter()
            .collect(),
            rejected: [(mask_id.clone(), PolicyRejection::Masked)]
                .into_iter()
                .collect(),
            blockers: vec![ActiveBlocker {
                owner: effective,
                atom: "app-misc/baz".parse().unwrap(),
            }],
            removals: [installed.clone()].into_iter().collect(),
            replacements: [(foo_id, installed)].into_iter().collect(),
        };
        let checkpoint = ResolverState::default().checkpoint();

        state.rollback(checkpoint);

        assert!(state.selected.is_empty());
        assert!(state.visiting.is_empty());
        assert!(state.blockers.is_empty());
        assert!(state.removals.is_empty());
        assert!(state.replacements.is_empty());
        assert_eq!(state.rejection(&mask_id), Some(&PolicyRejection::Masked));
    }
}
