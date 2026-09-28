use std::cmp::Ordering;

use super::outcome::{CandidateRejection, ResolutionOutcome, SelectedPackage};
use super::{ExecutionPlan, PackageOperation};
use crate::atom::Atom;
use crate::package::{AtomRequirement, Package, PackageView, cpv::CPV};
use crate::repository::RepoName;
use crate::types::{FxHashMap, FxIndexMap, FxIndexSet};
use crate::useflag::EffectiveUse;
use crate::vdb::package::InstalledPackage;

/// Holds the current resolver state.
///
/// All blockers must be resolved in the final state.
#[derive(Default, Debug)]
pub struct ResolverState {
    /// Holds packages that have been selected for installation.
    selected: FxIndexMap<PackageKey, SelectedPackage>,
    /// Holds temporary packages that are currently being visited.
    visiting: FxHashMap<PackageKey, SelectedPackage>,
    /// Holds packages that have been rejected due to policy or unsatisfied dependencies.
    rejected: FxIndexMap<PackageKey, CandidateRejection>,
    /// Holds weak blockers that need to be resolved.
    blockers: Vec<ActiveBlocker>,
    /// Holds installed packages that are planned for removal.
    removals: FxIndexSet<InstalledPackage>,
    /// Holds installed package versions that should be replaced.
    replacements: FxIndexMap<PackageKey, InstalledPackage>,
}

impl ResolverState {
    /// Returns whether a selected or visiting package with `key`
    /// satisfies the given `request`.
    pub fn matches_key(
        &self,
        key: &PackageKey,
        request: &AtomRequirement<'_>,
    ) -> anyhow::Result<bool> {
        match self.selected.get(key).or_else(|| self.visiting.get(key)) {
            Some(entry) => entry.matches(request),
            None => Ok(false),
        }
    }

    /// Marks the package for the given `key` as selected.
    pub fn select(&mut self, key: PackageKey) {
        let pkg = self
            .visiting
            .remove(&key)
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
    pub fn visiting(&mut self, key: PackageKey, pkg: Package, effective_use: EffectiveUse) {
        self.visiting
            .insert(key, SelectedPackage::new(pkg, effective_use));
    }

    /// Marks a package as rejected.
    pub fn reject(&mut self, key: PackageKey, reason: CandidateRejection) {
        self.visiting.remove(&key);
        self.rejected.insert(key, reason);
    }

    /// Checks if a package with the given `key` has been rejected.
    pub fn is_rejected(&self, key: &PackageKey) -> bool {
        self.rejected.contains_key(key)
    }

    /// Applies a new blocker to the resolver state.
    pub fn insert_blocker(&mut self, owner_use: EffectiveUse, atom: Atom) {
        self.blockers.push(ActiveBlocker { owner_use, atom });
    }

    /// Records `pkg` as planned for removal.
    pub fn insert_removal(&mut self, pkg: InstalledPackage) {
        self.removals.insert(pkg);
    }

    /// Records `installed` as replaced by the selected package with `key`.
    pub fn insert_replacement(&mut self, key: PackageKey, installed: InstalledPackage) {
        self.replacements.insert(key, installed);
    }

    /// Checks if the given `pkg` is blocked by any active blockers.
    pub fn is_blocked(&self, pkg: &Package, target_use: &EffectiveUse) -> anyhow::Result<bool> {
        for blocker in &self.blockers {
            if blocker.matches(pkg, target_use)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Returns an iterator over all selected and packages being currently visited.
    pub fn selected(&self) -> impl Iterator<Item = (&PackageKey, &SelectedPackage)> {
        self.selected.iter().chain(self.visiting.iter())
    }

    /// Consumes and finalizes the state to produce a [`ResolutionOutcome`].
    pub fn finalize(mut self, is_resolved: bool) -> ResolutionOutcome {
        let mut operations = Vec::with_capacity(self.selected.len() + self.removals.len());
        for (key, sel) in self.selected {
            let op = match self.replacements.shift_remove(&key) {
                Some(installed) => match sel.pkg.cpv().cmp(installed.cpv()) {
                    Ordering::Equal => PackageOperation::Replace(sel, installed),
                    Ordering::Greater => PackageOperation::Upgrade(sel, installed),
                    Ordering::Less => PackageOperation::Downgrade(sel, installed),
                },
                None => PackageOperation::Merge(sel),
            };
            operations.push(op);
        }
        for pkg in self.removals {
            operations.push(PackageOperation::Unmerge(pkg));
        }

        ResolutionOutcome::new(is_resolved, ExecutionPlan::new(operations), self.rejected)
    }
}

/// Represents a unique key for a package based on its CPV and repo name.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct PackageKey((CPV, RepoName));

impl PackageKey {
    pub fn new<P: PackageView>(pkg: &P) -> Self {
        Self((pkg.cpv().clone(), pkg.repo().clone()))
    }
}

/// Defines an active weak blocker in a dependency expression.
#[derive(Clone, Debug)]
struct ActiveBlocker {
    owner_use: EffectiveUse,
    atom: Atom,
}

impl ActiveBlocker {
    fn matches(&self, package: &Package, target_use: &EffectiveUse) -> anyhow::Result<bool> {
        AtomRequirement::dependency(&self.atom, &self.owner_use).satisfied_by(package, target_use)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::pkg;

    fn state_with_pkg(pkg: &Package, target_use: EffectiveUse) -> ResolverState {
        let mut state = ResolverState::default();
        state.visiting(PackageKey::new(pkg), pkg.clone(), target_use);
        state
    }

    #[test]
    fn test_reject() {
        let pkg = pkg("app-misc", "foo", "1.0", &[]);
        let key = PackageKey::new(&pkg);
        let mut state = state_with_pkg(&pkg, EffectiveUse::default());

        state.reject(key.clone(), CandidateRejection::Masked);

        assert!(!state.is_visiting(&key));
        assert!(state.is_rejected(&key));
    }
}
