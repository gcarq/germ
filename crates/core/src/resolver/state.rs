use super::PackageKey;
use crate::atom::Atom;
use crate::package::{AtomRequirement, Package};
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::EffectiveUse;

/// Holds the current resolver state.
///
/// All blockers must be resolved in the final state.
#[derive(Clone, Default, Debug)]
pub struct ResolverState {
    /// Holds packages that have been selected for installation.
    selected: FxHashMap<PackageKey, ResolvedPackage>,
    /// Holds temporary packages that are currently being visited.
    visiting: FxHashMap<PackageKey, ResolvedPackage>,
    /// Holds packages that have been rejected due to policy or unsatisfied dependencies.
    rejected: FxHashSet<PackageKey>,
    /// Holds weak blockers that need to be resolved.
    blockers: Vec<ActiveBlocker>,
}

impl ResolverState {
    /// Returns the first selected or visiting package, other than `exclude`,
    /// that satisfies the `request`.
    pub fn first_match(
        &self,
        request: &AtomRequirement<'_>,
        exclude: Option<&PackageKey>,
    ) -> anyhow::Result<Option<&Package>> {
        for (key, resolved) in self.selected.iter().chain(self.visiting.iter()) {
            if exclude == Some(key) {
                continue;
            }
            if resolved.matches(request)? {
                return Ok(Some(&resolved.package));
            }
        }
        Ok(None)
    }

    /// Returns whether a selected or visiting package with `key`
    /// satisfies the given `request`.
    pub fn matches_key(
        &self,
        key: &PackageKey,
        request: &AtomRequirement<'_>,
    ) -> anyhow::Result<bool> {
        match self.selected.get(key).or_else(|| self.visiting.get(key)) {
            Some(resolved) => resolved.matches(request),
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
        self.visiting.insert(
            key,
            ResolvedPackage {
                package: pkg,
                effective_use,
            },
        );
    }

    /// Marks a package as rejected.
    pub fn reject(&mut self, key: PackageKey) {
        self.visiting.remove(&key);
        self.rejected.insert(key);
    }

    /// Checks if a package with the given `key` has been rejected.
    pub fn is_rejected(&self, key: &PackageKey) -> bool {
        self.rejected.contains(key)
    }

    /// Applies a new blocker to the resolver state.
    pub fn insert_blocker(&mut self, owner_use: EffectiveUse, atom: Atom) {
        self.blockers.push(ActiveBlocker { owner_use, atom });
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
}

#[derive(Clone, Debug)]
struct ResolvedPackage {
    package: Package,
    effective_use: EffectiveUse,
}

impl ResolvedPackage {
    fn matches(&self, request: &AtomRequirement<'_>) -> anyhow::Result<bool> {
        request.satisfied_by(&self.package, &self.effective_use)
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
    use crate::useflag::test_support::effective;

    fn state_with_pkg(pkg: &Package, target_use: EffectiveUse) -> ResolverState {
        let mut state = ResolverState::default();
        state.visiting(PackageKey::new(pkg), pkg.clone(), target_use);
        state
    }

    #[test]
    fn test_reject() {
        let pkg = pkg("app-misc", "foo", "1.0", &[]);
        let key = PackageKey::new(&pkg);
        let mut state = state_with_pkg(&pkg, effective(&[], &[]));

        state.reject(key.clone());

        assert!(!state.is_visiting(&key));
        assert!(state.is_rejected(&key));
    }

    #[test]
    fn test_first_match() {
        let pkg = pkg("app-misc", "foo", "1.0", &[]);
        let state = state_with_pkg(&pkg, effective(&[], &[]));
        let atom = "app-misc/foo".parse().unwrap();
        let owner_use = effective(&[], &[]);
        let request = AtomRequirement::dependency(&atom, &owner_use);

        assert!(state.first_match(&request, None).unwrap().is_some());
    }

    #[test]
    fn test_first_match_excludes() {
        let pkg = pkg("app-misc", "foo", "1.0", &[]);
        let key = PackageKey::new(&pkg);
        let state = state_with_pkg(&pkg, effective(&[], &[]));
        let atom = "app-misc/foo".parse().unwrap();
        let owner_use = effective(&[], &[]);
        let request = AtomRequirement::dependency(&atom, &owner_use);

        assert!(state.first_match(&request, Some(&key)).unwrap().is_none());
    }
}
