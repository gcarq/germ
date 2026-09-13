use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::deps::atom::Atom;
use crate::package::cpv::CPV;
use crate::package::names::{CatName, PkgName};
use crate::types::{FxHashMap, FxHashSet};

use either::Either;

/// Holds all available packages in a repository, grouped by category and package name.
#[derive(Default, Debug)]
pub struct CPVIndex {
    state: RwLock<IndexState>,
}

impl CPVIndex {
    /// Updates the cache with the given `cpvs` and marks `atom` as discovered.
    ///
    /// Already cached [`CPV`] will be ignored.
    pub fn update(&self, atom: &Atom, cpvs: impl IntoIterator<Item = CPV>) {
        let mut state = self.write();
        for cpv in cpvs {
            state.insert(cpv);
        }
        state.discovery.mark_discovered(atom);
    }

    /// Returns all indexed CPVs.
    pub fn entries(&self) -> Vec<CPV> {
        self.read().iter().cloned().collect()
    }

    /// Clears the index.
    pub fn clear(&self) {
        self.write().clear();
    }

    /// Returns all cached CPVs matching the given [`Atom`].
    pub fn matching_entries(&self, atom: &Atom) -> Vec<CPV> {
        self.read().matching_entries(atom).cloned().collect()
    }

    /// Checks if all CPVs matching the given [`Atom`] have been discovered.
    pub fn is_discovered(&self, atom: &Atom) -> bool {
        self.read().discovery.is_discovered(atom)
    }

    /// Returns a write lock on the index state.
    fn write(&self) -> RwLockWriteGuard<'_, IndexState> {
        self.state
            .write()
            .expect("failed to acquire write lock on CPVIndex")
    }

    /// Returns a read lock on the index state.
    fn read(&self) -> RwLockReadGuard<'_, IndexState> {
        self.state
            .read()
            .expect("failed to acquire read lock on CPVIndex")
    }
}

#[derive(Default, Debug)]
struct IndexState {
    indexed: FxHashMap<CatName, FxHashMap<PkgName, Vec<CPV>>>,
    discovery: DiscoveryState,
}

impl IndexState {
    /// Inserts the given [`CPV`].
    ///
    /// Already known entries will be ignored.
    fn insert(&mut self, cpv: CPV) {
        match self.indexed.get_mut(cpv.category()) {
            Some(pkgs) => match pkgs.get_mut(cpv.package()) {
                Some(existing) => match existing.binary_search_by(|e| e.cmp(&cpv).reverse()) {
                    Ok(_) => {} // already present
                    Err(pos) => existing.insert(pos, cpv),
                },
                None => {
                    pkgs.insert(cpv.package().clone(), vec![cpv]);
                }
            },
            None => {
                self.indexed.insert(
                    cpv.category().clone(),
                    FxHashMap::from_iter([(cpv.package().clone(), vec![cpv])]),
                );
            }
        }
    }

    /// Returns all cached CPVs matching the given [`Atom`].
    fn matching_entries(&self, atom: &Atom) -> impl Iterator<Item = &CPV> {
        let iter = match (atom.category(), atom.package()) {
            (Some(cat), Some(pkg)) => Either::Left(
                self.indexed
                    .get(cat)
                    .into_iter()
                    .filter_map(|pkgs| pkgs.get(pkg))
                    .flat_map(|cpvs| cpvs.iter()),
            ),
            (Some(cat), None) => Either::Right(Either::Left(
                self.indexed
                    .get(cat)
                    .into_iter()
                    .flat_map(|pkgs| pkgs.values())
                    .flat_map(|cpvs| cpvs.iter()),
            )),
            (None, Some(pkg)) => Either::Right(Either::Right(Either::Left(
                self.indexed
                    .values()
                    .filter_map(|pkgs| pkgs.get(pkg))
                    .flat_map(|cpvs| cpvs.iter()),
            ))),
            (None, None) => Either::Right(Either::Right(Either::Right(self.iter()))),
        };
        iter.filter(|cpv| atom.matches(cpv))
    }

    /// Returns all indexed CPVs.
    fn iter(&self) -> impl Iterator<Item = &CPV> {
        self.indexed
            .values()
            .flat_map(|pkgs| pkgs.values())
            .flat_map(|cpvs| cpvs.iter())
    }

    /// Clears the index.
    fn clear(&mut self) {
        self.discovery.clear();
        self.indexed.clear();
    }
}

/// Defines the discovery state for [`CPVIndex`].
///
/// When lazy loading the index we must be able to distinguish
/// from CPVs that are not loaded yet or just don't exist.
#[derive(Default, Debug)]
struct DiscoveryState {
    all: bool,                                        // Atoms `*/*` and `*/package`
    categories: FxHashSet<CatName>,                   // Atom `category/*`
    packages: FxHashMap<CatName, FxHashSet<PkgName>>, // Atom `category/package`
}

impl DiscoveryState {
    /// Checks if the given [`Atom`] has already been discovered.
    fn is_discovered(&self, atom: &Atom) -> bool {
        if self.all {
            return true;
        }

        match (atom.category(), atom.package()) {
            (Some(cat), Some(pkg)) => {
                self.categories.contains(cat)
                    || self
                        .packages
                        .get(cat)
                        .is_some_and(|pkgs| pkgs.contains(pkg))
            }
            (Some(cat), None) => self.categories.contains(cat),
            (None, _) => false,
        }
    }

    /// Marks the given [`Atom`] as discovered.
    fn mark_discovered(&mut self, atom: &Atom) {
        match atom.category() {
            None => self.all = true,
            Some(cat) => match atom.package() {
                None => {
                    self.categories.insert(cat.clone());
                }
                Some(pkg) => match self.packages.get_mut(cat) {
                    Some(existing) => {
                        existing.insert(pkg.clone());
                    }
                    None => {
                        self.packages
                            .insert(cat.clone(), FxHashSet::from_iter([pkg.clone()]));
                    }
                },
            },
        }
    }

    /// Clears the discovery state, but keeps the memory allocated.
    fn clear(&mut self) {
        self.all = false;
        self.categories.clear();
        self.packages.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::cpv;

    #[test]
    fn test_discovery_coverage() {
        // discovered, query, is_discovered
        let test_cases = [
            ("*/*", "dev-lang/rust", true),
            ("dev-lang/*", "dev-lang/rust", true),
            ("dev-lang/*", "dev-libs/rust", false),
            ("*/rust", "dev-lang/rust", true),
            ("*/rust", "dev-lang/python", true),
            ("dev-lang/rust", "dev-lang/rust", true),
            ("dev-lang/rust", "dev-lang/python", false),
        ];

        for (discovered, queried, expected) in test_cases {
            let index = CPVIndex::default();
            index.update(&Atom::new(discovered).unwrap(), []);
            assert_eq!(
                index.is_discovered(&Atom::new(queried).unwrap()),
                expected,
                "{discovered} -> {queried}"
            );
        }
    }

    #[test]
    fn test_matching_entries_exact() {
        let index = CPVIndex::default();
        let python_3_13 = cpv("dev-lang", "python", "3.13.12");
        let python3_14 = cpv("dev-lang", "python", "3.14.3");
        let rust = cpv("dev-lang", "rust", "1.94.0");
        index.update(
            &Atom::default(),
            [python_3_13.clone(), python3_14.clone(), rust.clone()],
        );
        assert!(index.entries().into_iter().any(|existing| existing == rust));

        let atom = Atom::new("=dev-lang/python-3.13.12").unwrap();
        assert_eq!(index.matching_entries(&atom), vec![python_3_13.clone()]);

        let atom = Atom::new("dev-lang/python").unwrap();
        assert_eq!(index.matching_entries(&atom), vec![python3_14, python_3_13]);
    }

    #[test]
    fn test_matching_entries_wildcards() {
        let index = CPVIndex::default();
        index.update(
            &Atom::default(),
            [
                cpv("dev-lang", "python", "3.14.3"),
                cpv("dev-lang", "rust", "1.94.0"),
                cpv("dev-libs", "libfoo", "1.0.0"),
            ],
        );

        let atom = Atom::new("dev-lang/*").unwrap();
        assert_eq!(index.matching_entries(&atom).len(), 2);
        let atom = Atom::new("*/rust").unwrap();
        assert_eq!(index.matching_entries(&atom).len(), 1);
        let atom = Atom::new("*/*").unwrap();
        assert_eq!(index.matching_entries(&atom).len(), 3);
    }

    #[test]
    fn test_matching_entries_equal_versions() {
        let implicit = cpv("dev-libs", "pkg", "1.0");
        let explicit = cpv("dev-libs", "pkg", "1.0-r0");
        let r1 = cpv("dev-libs", "pkg", "1.0-r1");

        let index = CPVIndex::default();
        index.update(&Atom::default(), [explicit, implicit, r1]);

        let atom = Atom::new("dev-libs/pkg").unwrap();
        assert_eq!(index.matching_entries(&atom).len(), 2);
    }
}
