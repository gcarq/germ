use crate::deps::atom::Atom;
use crate::package::cpv::CPV;
use crate::package::names::{CatName, PkgName};
use crate::types::{FxHashMap, FxHashSet};

use either::Either;
use log::warn;

/// Holds all available packages in a repository, grouped by category and package name.
#[derive(Default, Debug)]
pub struct CPVIndex {
    index: FxHashMap<CatName, FxHashMap<PkgName, Vec<CPV>>>,
    discovery_state: DiscoveryState,
}

impl CPVIndex {
    /// Inserts the given [`CPV`] into the index.
    ///
    /// NOTE: The caller must ensure to call [`CPVIndex::sort`] after all insertions are done.
    pub fn insert(&mut self, cpvs: impl IntoIterator<Item = CPV>) {
        for cpv in cpvs {
            let packages = match self.index.get_mut(cpv.category()) {
                Some(packages) => packages,
                None => self.index.entry(cpv.category().clone()).or_default(),
            };
            match packages.get_mut(cpv.package()) {
                Some(cpvs) => cpvs.push(cpv),
                None => packages.entry(cpv.package().clone()).or_default().push(cpv),
            }
        }
    }

    /// Sorts all [`CPV`] values in the index by version in descending order and removes duplicates.
    pub fn sort(&mut self) {
        for cpvs in self.index.values_mut().flat_map(|pkgs| pkgs.values_mut()) {
            cpvs.sort_unstable_by(|a, b| b.cmp(a));
            cpvs.dedup_by(|cur, prev| match cur == prev {
                true => {
                    warn!("collision between {} and {}", prev.pf(), cur.pf());
                    true
                }
                false => false,
            });
        }
    }

    /// Returns all indexed [`CPV`] values.
    pub fn iter(&self) -> impl Iterator<Item = &CPV> {
        self.index
            .values()
            .flat_map(|packages| packages.values())
            .flat_map(|cpvs| cpvs.iter())
    }

    /// Clears the index.
    pub fn clear(&mut self) {
        self.discovery_state.clear();
        self.index.clear();
    }

    /// Returns all packages matching the given [`Atom`].
    pub fn find_packages(&self, atom: &Atom) -> impl Iterator<Item = &CPV> {
        let matches = move |cpv: &&CPV| atom.matches(cpv);

        match (atom.category(), atom.package()) {
            (Some(category), Some(package)) => Either::Left(
                self.index
                    .get(category)
                    .into_iter()
                    .filter_map(|pkgs| pkgs.get(package))
                    .flat_map(|cpvs| cpvs.iter())
                    .filter(matches),
            ),
            (Some(category), None) => Either::Right(Either::Left(
                self.index
                    .get(category)
                    .into_iter()
                    .flat_map(|pkgs| pkgs.values())
                    .flat_map(|cpvs| cpvs.iter())
                    .filter(matches),
            )),
            (None, Some(package)) => Either::Right(Either::Right(Either::Left(
                self.index
                    .values()
                    .filter_map(|pkgs| pkgs.get(package))
                    .flat_map(|cpvs| cpvs.iter())
                    .filter(matches),
            ))),
            (None, None) => Either::Right(Either::Right(Either::Right(self.iter()))),
        }
    }

    pub fn is_discovered(&self, atom: &Atom) -> bool {
        self.discovery_state.is_discovered(atom)
    }

    pub fn mark_discovered(&mut self, atom: &Atom) {
        self.discovery_state.mark_discovered(atom);
    }
}

/// Defines the discovery state for [`CPVIndex`].
///
/// When lazy loading the index we must be able to distinguish
/// from CPVs that are not loaded yet or just don't exist.
///
/// TODO: It might make sense to separate `*/package` from `*/*`.
#[derive(Default, Debug)]
struct DiscoveryState {
    all: bool,                                        // Atoms `*/*` and `*/package`
    categories: FxHashSet<CatName>,                   // Atom `category/*`
    packages: FxHashMap<CatName, FxHashSet<PkgName>>, // Atom `category/package`
}

impl DiscoveryState {
    /// Checks if the given [`Atom`] has already been discovered.
    pub fn is_discovered(&self, atom: &Atom) -> bool {
        if self.all {
            return true;
        }

        match (atom.category(), atom.package()) {
            (Some(cat), _) if self.categories.contains(cat) => true,
            (Some(cat), Some(pkg)) => self
                .packages
                .get(cat)
                .is_some_and(|pkgs| pkgs.contains(pkg)),
            _ => false,
        }
    }

    /// Marks the given [`Atom`] as discovered.
    pub fn mark_discovered(&mut self, atom: &Atom) {
        match (atom.category(), atom.package()) {
            (Some(cat), Some(pkg)) => {
                self.packages
                    .entry(cat.clone())
                    .or_default()
                    .insert(pkg.clone());
            }
            (Some(cat), None) => {
                self.categories.insert(cat.clone());
            }
            (None, _) => self.all = true,
        }
    }

    /// Clears the discovery state, but keeps the memory allocated.
    pub fn clear(&mut self) {
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
    fn test_discovery_state_is_discovered() {
        // discovered, query, is_discovered
        let test_cases = [
            ("*/*", "dev-lang/rust", true),
            ("dev-lang/*", "dev-lang/rust", true),
            ("dev-lang/*", "dev-libs/rust", false),
            ("*/rust", "dev-lang/rust", true),
            ("dev-lang/rust", "dev-lang/rust", true),
            ("dev-lang/rust", "dev-lang/python", false),
        ];

        for (discovered, queried, expected) in test_cases {
            let mut state = DiscoveryState::default();
            state.mark_discovered(&Atom::new(discovered).unwrap());
            assert_eq!(
                state.is_discovered(&Atom::new(queried).unwrap()),
                expected,
                "{discovered} -> {queried}"
            );
        }
    }

    #[test]
    fn test_available_package_index() {
        let mut index = CPVIndex::default();
        let python_3_13 = cpv("dev-lang", "python", "3.13.12");
        let python3_14 = cpv("dev-lang", "python", "3.14.3");
        let rust = cpv("dev-lang", "rust", "1.94.0");
        index.insert([python_3_13.clone(), python3_14.clone(), rust.clone()]);
        index.sort();
        assert!(index.iter().any(|existing| existing == &rust));

        let packages = index
            .find_packages(&Atom::new("dev-lang/python").unwrap())
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(packages, vec![python3_14, python_3_13]);
    }

    #[test]
    fn test_available_package_index_wildcards() {
        let mut index = CPVIndex::default();
        index.insert([
            cpv("dev-lang", "python", "3.14.3"),
            cpv("dev-lang", "rust", "1.94.0"),
            cpv("dev-libs", "libfoo", "1.0.0"),
        ]);
        index.sort();

        let atom = Atom::new("dev-lang/*").unwrap();
        assert_eq!(index.find_packages(&atom).count(), 2);
        let atom = Atom::new("*/rust").unwrap();
        assert_eq!(index.find_packages(&atom).count(), 1);
        let atom = Atom::new("*/*").unwrap();
        assert_eq!(index.find_packages(&atom).count(), 3);
    }

    #[test]
    fn test_available_package_index_r0_collision() {
        let implicit = cpv("dev-libs", "pkg", "1.0");
        let explicit = cpv("dev-libs", "pkg", "1.0-r0");
        let r1 = cpv("dev-libs", "pkg", "1.0-r1");

        let mut index = CPVIndex::default();
        index.insert([explicit, implicit, r1]);
        index.sort();

        let atom = Atom::new("dev-libs/pkg").unwrap();
        assert_eq!(index.find_packages(&atom).count(), 2);
    }
}
