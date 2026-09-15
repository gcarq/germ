use crate::package::cpv::CPV;
use crate::package::metadata::{PackageMetadata, RawPackageMetadata};
use crate::package::version::PackageVersion;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};

/// Creates a CPV from valid test fixture strings.
pub fn cpv(category: &str, package: &str, version: &str) -> CPV {
    CPV::new(
        category.parse().unwrap(),
        package.parse().unwrap(),
        PackageVersion::try_from(version).unwrap(),
    )
}

/// Creates valid package metadata with optional variable overrides.
pub fn package_metadata(values: &[(&str, &str)]) -> PackageMetadata {
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
