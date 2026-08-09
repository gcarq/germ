use crate::deps::atom::{Atom, AtomIdent};
use crate::package::cpv::CPV;
use crate::types::FxHashMap;

use either::Either;
use log::warn;

type PackagesByName = FxHashMap<Box<str>, Vec<CPV>>;

/// Holds all available packages in a repository, grouped by category and package name.
#[derive(Default, Debug)]
pub struct CPVIndex(FxHashMap<Box<str>, PackagesByName>);

impl CPVIndex {
    /// Inserts the given [`CPV`] into the index.
    pub fn insert(&mut self, cpv: CPV) {
        let packages = match self.0.get_mut(cpv.category()) {
            Some(packages) => packages,
            None => self.0.entry(cpv.category().into()).or_default(),
        };
        let entry = match packages.get_mut(cpv.package()) {
            Some(entry) => entry,
            None => packages.entry(cpv.package().into()).or_default(),
        };

        // Its possible that a repository contains the same ebuild with and without explicit
        // revision 0.
        if let Some(index) = entry.iter().position(|existing| existing == &cpv) {
            warn!(
                "Ignoring equal version collision between {} and {}",
                entry[index].pf(),
                cpv.pf()
            );
            return;
        }
        entry.push(cpv);
        entry.sort_unstable_by(|a, b| b.cmp(a));
    }

    pub fn insert_all(&mut self, cpvs: Vec<CPV>) {
        for cpv in cpvs {
            self.insert(cpv);
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
                    .get(category.as_str())
                    .into_iter()
                    .filter_map(|pkgs| pkgs.get(package.as_str()))
                    .flat_map(|cpvs| cpvs.iter())
                    .filter(matches),
            ),
            (AtomIdent::Exact(category), AtomIdent::Any) => Either::Right(Either::Left(
                self.0
                    .get(category.as_str())
                    .into_iter()
                    .flat_map(|pkgs| pkgs.values())
                    .flat_map(|cpvs| cpvs.iter())
                    .filter(matches),
            )),
            (AtomIdent::Any, AtomIdent::Exact(package)) => {
                Either::Right(Either::Right(Either::Left(
                    self.0
                        .values()
                        .filter_map(|pkgs| pkgs.get(package.as_str()))
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
    use crate::package::version::PackageVersion;

    #[test]
    fn test_available_package_index() {
        let mut index = CPVIndex::default();
        let python_3_13 = CPV::new(
            "dev-lang",
            "python",
            PackageVersion::try_from("3.13.12").unwrap(),
        )
        .unwrap();
        index.insert(python_3_13.clone());
        let python3_14 = CPV::new(
            "dev-lang",
            "python",
            PackageVersion::try_from("3.14.3").unwrap(),
        )
        .unwrap();
        index.insert(python3_14.clone());

        let rust = CPV::new(
            "dev-lang",
            "rust",
            PackageVersion::try_from("1.94.0").unwrap(),
        )
        .unwrap();
        assert!(!index.iter().any(|existing| existing == &rust));
        index.insert(rust.clone());

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
        index.insert(
            CPV::new(
                "dev-lang",
                "python",
                PackageVersion::try_from("3.14.3").unwrap(),
            )
            .unwrap(),
        );
        index.insert(
            CPV::new(
                "dev-lang",
                "rust",
                PackageVersion::try_from("1.94.0").unwrap(),
            )
            .unwrap(),
        );
        index.insert(
            CPV::new(
                "dev-libs",
                "libfoo",
                PackageVersion::try_from("1.0.0").unwrap(),
            )
            .unwrap(),
        );

        let atom = Atom::new("dev-lang/*").unwrap();
        assert_eq!(index.find_packages(&atom).count(), 2);
        let atom = Atom::new("*/rust").unwrap();
        assert_eq!(index.find_packages(&atom).count(), 1);
        let atom = Atom::new("*/*").unwrap();
        assert_eq!(index.find_packages(&atom).count(), 3);
    }

    #[test]
    fn test_available_package_index_r0_collision() {
        let implicit =
            CPV::new("dev-libs", "pkg", PackageVersion::try_from("1.0").unwrap()).unwrap();
        let explicit = CPV::new(
            "dev-libs",
            "pkg",
            PackageVersion::try_from("1.0-r0").unwrap(),
        )
        .unwrap();
        let r1 = CPV::new(
            "dev-libs",
            "pkg",
            PackageVersion::try_from("1.0-r1").unwrap(),
        )
        .unwrap();

        let mut index = CPVIndex::default();
        index.insert_all(vec![explicit, implicit, r1]);

        let atom = Atom::new("dev-libs/pkg").unwrap();
        assert_eq!(index.find_packages(&atom).count(), 2);
    }
}
