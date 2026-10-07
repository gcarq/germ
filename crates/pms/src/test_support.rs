#![allow(clippy::missing_panics_doc)]

use std::collections::hash_map::DefaultHasher;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};

use crate::atom::Atom;
use crate::package::{
    CPV, Package, PackageMetadata, PackageVersion, PackageView, RawPackageMetadata,
};

pub fn cpv(category: &str, package: &str, version: &str) -> CPV {
    CPV::new(
        category.parse().unwrap(),
        package.parse().unwrap(),
        PackageVersion::try_from(version).unwrap(),
    )
}

/// Creates a [`Package`] from valid strings for testing.
pub fn pkg(cat: &str, name: &str, version: &str, metadata: &[(&str, &str)]) -> Package {
    Package::new(
        cpv(cat, name, version),
        "gentoo".parse().unwrap(),
        pkg_metadata(metadata),
    )
}

/// Creates valid package metadata with optional variable overrides.
pub fn pkg_metadata(values: &[(&str, &str)]) -> PackageMetadata {
    let defaults = [
        ("EAPI", "8"),
        ("DESCRIPTION", "Some description"),
        ("SLOT", "0"),
    ];
    let vars = defaults
        .into_iter()
        .chain(values.iter().copied())
        .collect::<RawPackageMetadata<'_>>();
    PackageMetadata::from_raw(&vars, None).unwrap()
}

/// Asserts that `pkg` matches the atoms it should and rejects the atoms it should not.
pub fn assert_package_view_matches_atoms<P: PackageView>(pkg: &P) {
    for atom in [
        "sys-devel/gcc",
        "sys-devel/gcc::gentoo",
        "=sys-devel/gcc-15*",
        "sys-devel/gcc:15",
        "sys-devel/gcc:15=",
        "sys-devel/gcc:15/15",
        "sys-devel/gcc:15/15=",
        "sys-devel/gcc:*",
        "sys-devel/gcc:=",
    ] {
        let atom = Atom::new(atom).unwrap();
        assert!(pkg.matches_atom(&atom), "{atom} should match");
    }

    for atom in [
        "sys-devel/gcc::local",
        "sys-devel/binutils",
        "virtual/gcc",
        "<sys-devel/gcc-15",
        "sys-devel/gcc:14",
        "sys-devel/gcc:14=",
        "sys-devel/gcc:15/0",
        "sys-devel/gcc:15/0=",
    ] {
        let atom = Atom::new(atom).unwrap();
        assert!(!pkg.matches_atom(&atom), "{atom} shouldn't match");
    }
}

/// Asserts that two values are equal and that their hashes are equal as well.
pub fn assert_eq_hash<T: Eq + Hash + Debug>(a: &T, b: &T) {
    assert_eq!(a, b, "values {a:?} and {b:?} are not equal");
    assert_eq!(hash(a), hash(b), "hashes of {a:?} and {b:?} are not equal");
}

fn hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
