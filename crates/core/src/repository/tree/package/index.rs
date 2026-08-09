use crate::deps::atom::Atom;
use crate::package::cpv::CPV;
use crate::types::FxHashMap;

use log::warn;
use std::ops::Deref;

/// Holds all available packages in a repository, mapping the qualified name to [`Vec<CPV>`].
#[derive(Default, Debug)]
pub struct CPVIndex(FxHashMap<String, Vec<CPV>>);

impl CPVIndex {
    /// Inserts the given [`CPV`] into the index.
    pub fn insert(&mut self, cpv: CPV) {
        let entry = self.0.entry(cpv.qualified_name()).or_default();
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

    /// Returns all packages matching the given [`Atom`].
    pub fn find_packages(&self, atom: &Atom) -> Vec<CPV> {
        let Some(pkgs) = self.0.get(&atom.qualified_name()) else {
            return Vec::new();
        };
        pkgs.iter()
            .filter(|cpv| cpv.matches_atom(atom))
            .cloned()
            .collect()
    }
}

impl Deref for CPVIndex {
    type Target = FxHashMap<String, Vec<CPV>>;
    fn deref(&self) -> &Self::Target {
        &self.0
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
        assert!(!index.values().flatten().any(|existing| existing == &rust));
        index.insert(rust.clone());

        let packages = index.find_packages(&Atom::new("dev-lang/python").unwrap());
        assert_eq!(packages, vec![python3_14, python_3_13]);
        assert!(index.values().flatten().any(|existing| existing == &rust));
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
        assert_eq!(index["dev-libs/pkg"].len(), 2);
    }
}
