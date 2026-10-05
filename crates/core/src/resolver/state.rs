use std::cmp::Ordering;

use germ_pms::{Atom, CPV, RepoName};
use log::info;

use super::outcome::{RequirementFailure, ResolutionOutcome};
use super::{EffectivePackage, ExecutionPlan, PackageOperation};
use crate::package::{AtomRequirement, Package, PackageView};
use crate::policy::PolicyRejection;
use crate::types::{FxIndexMap, FxIndexSet};
use crate::vdb::package::InstalledPackage;

/// Holds the current resolver state.
///
/// All blockers must be resolved in the final state.
#[derive(Debug, Default)]
pub struct ResolverState {
    /// Holds packages that have been selected for installation.
    selected: FxIndexMap<PackageKey, EffectivePackage>,
    /// Holds temporary packages that are currently being visited.
    visiting: FxIndexMap<PackageKey, EffectivePackage>,
    /// Holds rejected candidates.
    rejected: FxIndexMap<PackageKey, PolicyRejection>,
    /// Holds weak blockers that need to be resolved.
    blockers: Vec<ActiveBlocker>,
    /// Holds installed packages that are planned for removal.
    removals: FxIndexSet<InstalledPackage>,
    /// Holds installed package versions that should be replaced.
    replacements: FxIndexMap<PackageKey, InstalledPackage>,
}

impl ResolverState {
    /// Returns whether a selected or visiting package with `key` satisfies `requirement`.
    pub fn satisfies(
        &self,
        key: &PackageKey,
        requirement: &AtomRequirement<'_>,
    ) -> anyhow::Result<bool> {
        self.selected
            .get(key)
            .or_else(|| self.visiting.get(key))
            .map_or(Ok(false), |pkg| pkg.matches(requirement))
    }

    /// Marks the package for the given `key` as selected.
    pub fn select(&mut self, key: PackageKey) {
        let pkg = self
            .visiting
            .shift_remove(&key)
            .expect("BUG: selected package must be visited");
        self.selected.insert(key, pkg);
    }

    /// Checks if a package with the given `key` is already selected.
    pub fn is_selected(&self, key: &PackageKey) -> bool {
        self.selected.contains_key(key)
    }

    /// Checks if a package with the given `key` is currently being visited.
    pub fn is_visiting(&self, key: &PackageKey) -> bool {
        self.visiting.contains_key(key)
    }

    /// Marks a package as being visited.
    pub fn visit(&mut self, key: PackageKey, candidate: &EffectivePackage) {
        self.visiting.insert(key, candidate.clone());
    }

    /// Marks a package as rejected.
    pub fn reject(&mut self, key: PackageKey, reason: PolicyRejection) {
        if !self.rejected.contains_key(&key) {
            self.rejected.insert(key, reason);
        }
    }

    /// Returns the candidate rejection for `key`.
    pub fn rejection(&self, key: &PackageKey) -> Option<&PolicyRejection> {
        self.rejected.get(key)
    }

    /// Returns the first selected or visiting package that conflicts with `atom`.
    pub fn weak_blocker_conflict(
        &self,
        owner: &EffectivePackage,
        atom: &Atom,
    ) -> anyhow::Result<Option<&Package>> {
        let requirement = AtomRequirement::dependency(atom, &owner.effective_use);
        let iter = self.selected.values().chain(self.visiting.values());
        for candidate in iter {
            if candidate.pkg != owner.pkg && candidate.matches(&requirement)? {
                return Ok(Some(&candidate.pkg));
            }
        }
        Ok(None)
    }

    /// Registers a weak blocker and plans removals for matching installed packages.
    pub fn register_weak_blocker(
        &mut self,
        owner: &EffectivePackage,
        atom: &Atom,
        installed: impl IntoIterator<Item = InstalledPackage>,
    ) -> anyhow::Result<()> {
        let requirement = AtomRequirement::dependency(atom, &owner.effective_use);
        for pkg in installed {
            if pkg.cpv() == owner.pkg.cpv() {
                continue;
            }
            if requirement.satisfied_by(&pkg, pkg.effective_use())? {
                info!(
                    "{}: planning removal of {pkg} for weak blocker {atom}",
                    owner.pkg
                );
                self.removals.insert(pkg);
            }
        }

        self.blockers.push(ActiveBlocker {
            owner: owner.clone(),
            atom: atom.clone(),
        });
        Ok(())
    }

    /// Returns the first active weak blocker that matches `candidate`.
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

    /// Records `installed` as replaced by the selected package with `key`.
    pub fn insert_replacement(&mut self, key: PackageKey, installed: InstalledPackage) {
        self.replacements.insert(key, installed);
    }

    /// Consumes the state and creates a [`ResolutionOutcome`].
    pub fn finalize(mut self, failure: Option<RequirementFailure>) -> ResolutionOutcome {
        let mut ops = Vec::with_capacity(self.selected.len() + self.removals.len());
        for (key, sel) in self.selected {
            let op = match self.replacements.shift_remove(&key) {
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

/// Represents a unique key for a package based on its CPV and repo name.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PackageKey((CPV, RepoName));

impl PackageKey {
    pub fn new<P: PackageView>(pkg: &P) -> Self {
        Self((pkg.cpv().clone(), pkg.repo().clone()))
    }
}

/// Defines an active weak blocker in a dependency expression.
#[derive(Clone, Debug)]
struct ActiveBlocker {
    owner: EffectivePackage,
    atom: Atom,
}

impl ActiveBlocker {
    fn matches(&self, candidate: &EffectivePackage) -> anyhow::Result<bool> {
        AtomRequirement::dependency(&self.atom, &self.owner.effective_use)
            .satisfied_by(&candidate.pkg, &candidate.effective_use)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::test_support::installed;
    use crate::test_support::pkg;
    use crate::useflag::EffectiveUse;
    use crate::useflag::test_support::effective;

    fn selected_state(package: Package, replacement: Option<InstalledPackage>) -> ResolverState {
        let key = PackageKey::new(&package);
        let mut state = ResolverState::default();
        state.visit(
            key.clone(),
            &EffectivePackage::new(package, EffectiveUse::default()),
        );
        state.select(key.clone());
        if let Some(installed) = replacement {
            state.insert_replacement(key, installed);
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
    fn test_finalize_unmerge() {
        let owner =
            EffectivePackage::new(pkg("app-misc", "owner", "1", &[]), EffectiveUse::default());
        let atom = "app-misc/foo".parse().unwrap();
        let installed = installed("foo", "1", "0", &[], &[]);
        let mut state = ResolverState::default();
        state
            .register_weak_blocker(&owner, &atom, [installed.clone()])
            .unwrap();
        let outcome = state.finalize(None);

        assert_eq!(
            outcome.plan().operations(),
            &[PackageOperation::Unmerge(installed)]
        );
    }

    #[test]
    fn test_weak_block_conflict_selected_before_visiting() {
        let owner = pkg("app-misc", "owner", "1", &[]);
        let selected = pkg("app-misc", "foo", "1", &[]);
        let visiting = pkg("app-misc", "foo", "2", &[]);
        let atom = "app-misc/foo".parse().unwrap();
        let mut state = ResolverState::default();

        let owner_key = PackageKey::new(&owner);
        state.visit(
            owner_key.clone(),
            &EffectivePackage::new(owner.clone(), EffectiveUse::default()),
        );
        state.select(owner_key);
        let selected_key = PackageKey::new(&selected);
        state.visit(
            selected_key.clone(),
            &EffectivePackage::new(selected.clone(), EffectiveUse::default()),
        );
        state.select(selected_key);
        state.visit(
            PackageKey::new(&visiting),
            &EffectivePackage::new(visiting, EffectiveUse::default()),
        );

        assert_eq!(
            state
                .weak_blocker_conflict(
                    &EffectivePackage::new(owner, EffectiveUse::default()),
                    &atom,
                )
                .unwrap(),
            Some(&selected)
        );
    }

    #[test]
    fn test_matching_active_blocker_first() {
        let owner = EffectivePackage::new(
            pkg("app-misc", "owner", "1", &[]),
            effective(&["feature"], &["feature"]),
        );
        let later_owner = EffectivePackage::new(
            pkg("app-misc", "later-owner", "1", &[]),
            effective(&["feature"], &["feature"]),
        );
        let target = EffectivePackage::new(
            pkg("app-misc", "target", "1", &[("IUSE", "feature")]),
            effective(&["feature"], &["feature"]),
        );
        let atom = "app-misc/target[feature?]".parse().unwrap();
        let mut state = ResolverState::default();
        state.register_weak_blocker(&owner, &atom, []).unwrap();
        state
            .register_weak_blocker(&later_owner, &atom, [])
            .unwrap();

        assert_eq!(
            state.matching_active_blocker(&target).unwrap(),
            Some((&atom, &owner.pkg))
        );
    }

    #[test]
    fn test_rollback_restores_state() {
        let foo = pkg("app-misc", "foo", "1", &[]);
        let bar = pkg("app-misc", "bar", "1", &[]);
        let mask = pkg("app-misc", "mask", "1", &[]);
        let foo_key = PackageKey::new(&foo);
        let mask_key = PackageKey::new(&mask);
        let installed = installed("foo", "0", "0", &[], &[]);
        let effective = EffectivePackage::new(foo, EffectiveUse::default());
        let mut state = ResolverState {
            selected: [(foo_key.clone(), effective.clone())].into_iter().collect(),
            visiting: [(
                PackageKey::new(&bar),
                EffectivePackage::new(bar, EffectiveUse::default()),
            )]
            .into_iter()
            .collect(),
            rejected: [(mask_key.clone(), PolicyRejection::Masked)]
                .into_iter()
                .collect(),
            blockers: vec![ActiveBlocker {
                owner: effective,
                atom: "app-misc/baz".parse().unwrap(),
            }],
            removals: [installed.clone()].into_iter().collect(),
            replacements: [(foo_key, installed)].into_iter().collect(),
        };
        let checkpoint = ResolverState::default().checkpoint();

        state.rollback(checkpoint);

        assert!(state.selected.is_empty());
        assert!(state.visiting.is_empty());
        assert!(state.blockers.is_empty());
        assert!(state.removals.is_empty());
        assert!(state.replacements.is_empty());
        assert_eq!(state.rejection(&mask_key), Some(&PolicyRejection::Masked));
    }
}
