use germ_pms::{CPV, PackageMetadata, PackageVersion, RawPackageMetadata};

use crate::package::Package;

/// Creates a CPV from valid test fixture strings.
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
