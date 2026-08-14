use crate::deps::atom::{Atom, AtomIdent};
use crate::package::cpv::CPV;
use crate::package::names::{CatName, PkgName};
use crate::types::FxHashMap;

use either::Either;
use log::warn;

/// Holds all available packages in a repository, grouped by category and package name.
#[derive(Default, Debug)]
pub struct CPVIndex(FxHashMap<CatName, FxHashMap<PkgName, Vec<CPV>>>);

impl CPVIndex {
    /// Inserts the given [`CPV`] into the index.
    ///
    /// NOTE: The caller must ensure to call [`CPVIndex::sort`] after all insertions are done.
    pub fn insert(&mut self, cpvs: impl IntoIterator<Item = CPV>) {
        for cpv in cpvs {
            let packages = match self.0.get_mut(cpv.category()) {
                Some(packages) => packages,
                None => self.0.entry(cpv.category().clone()).or_default(),
            };
            match packages.get_mut(cpv.package()) {
                Some(cpvs) => cpvs.push(cpv),
                None => packages.entry(cpv.package().clone()).or_default().push(cpv),
            }
        }
    }

    /// Sorts all [`CPV`] values in the index by version in descending order and removes duplicates.
    pub fn sort(&mut self) {
        for cpvs in self.0.values_mut().flat_map(|pkgs| pkgs.values_mut()) {
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
        self.0
            .values()
            .flat_map(|packages| packages.values())
            .flat_map(|cpvs| cpvs.iter())
    }

    /// Returns all packages matching the given [`Atom`].
    ///
    /// Wildcards for atom category and package are supported, see [`AtomIdent::Any`].
    pub fn find_packages(&self, atom: &Atom) -> impl Iterator<Item = &CPV> {
        let matches = move |cpv: &&CPV| cpv.matches_atom(atom);

        match (&atom.category, &atom.package) {
            (AtomIdent::Exact(category), AtomIdent::Exact(package)) => Either::Left(
                self.0
                    .get(category)
                    .into_iter()
                    .filter_map(|pkgs| pkgs.get(package))
                    .flat_map(|cpvs| cpvs.iter())
                    .filter(matches),
            ),
            (AtomIdent::Exact(category), AtomIdent::Any) => Either::Right(Either::Left(
                self.0
                    .get(category)
                    .into_iter()
                    .flat_map(|pkgs| pkgs.values())
                    .flat_map(|cpvs| cpvs.iter())
                    .filter(matches),
            )),
            (AtomIdent::Any, AtomIdent::Exact(package)) => {
                Either::Right(Either::Right(Either::Left(
                    self.0
                        .values()
                        .filter_map(|pkgs| pkgs.get(package))
                        .flat_map(|cpvs| cpvs.iter())
                        .filter(matches),
                )))
            }
            (AtomIdent::Any, AtomIdent::Any) => {
                Either::Right(Either::Right(Either::Right(self.iter())))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::cpv;

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
        assert!(index.iter().any(|existing| existing == &rust));
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
