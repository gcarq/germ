use std::fs;
use std::path::Path;

use germ_pms::{CatName, Cpv, PackageVersion, PkgName};
use log::debug;

/// Resolves all [`Cpv`] from the given repository path and categories.
pub fn resolve_from_repo_path<'r>(
    repo_path: &'r Path,
    categories: impl IntoIterator<Item = &'r CatName>,
) -> impl Iterator<Item = Cpv> {
    categories
        .into_iter()
        .flat_map(|cat| resolve_from_category_path(cat, &repo_path.join(cat.as_str())))
}

/// Resolves all [`Cpv`] from the given category path.
pub fn resolve_from_category_path<'c>(
    category: &'c CatName,
    cat_path: &Path,
) -> impl Iterator<Item = Cpv> + use<'c> {
    fs::read_dir(cat_path)
        .into_iter()
        .flatten()
        .filter_map(move |entry| {
            let entry = entry.ok()?;
            if !entry.file_type().ok()?.is_dir() {
                return None;
            }
            let package: PkgName = entry.file_name().into_string().ok()?.parse().ok()?;
            Some(resolve_from_pkg_path(category, package, &entry.path()))
        })
        .flatten()
}

/// Resolves all [`Cpv`] from the given package path.
pub fn resolve_from_pkg_path<'c>(
    category: &'c CatName,
    package: PkgName,
    pkg_path: &Path,
) -> impl Iterator<Item = Cpv> + use<'c> {
    fs::read_dir(pkg_path)
        .into_iter()
        .flatten()
        .filter_map(move |entry| {
            let entry = entry.ok()?;
            if !entry.file_type().ok()?.is_file() {
                return None;
            }

            let ebuild = entry.file_name().into_string().ok()?;
            cpv_from_fs_parts(category, package.clone(), &ebuild)
        })
}

/// Parses a [`Cpv`] from the given `category`, `package` and `ebuild_filename`.
///
/// Returns `None` if the file is not a valid ebuild or the package name doesn't match the ebuild name.
fn cpv_from_fs_parts(category: &CatName, package: PkgName, ebuild_filename: &str) -> Option<Cpv> {
    let Some(version) = ebuild_filename
        .strip_suffix(".ebuild")?
        .strip_prefix(package.as_str())
        .and_then(|rem| rem.strip_prefix('-'))
    else {
        debug!("ebuild is not in the correct directory: {category}/{package}/{ebuild_filename}");
        return None;
    };

    let version = PackageVersion::try_from(version).ok()?;
    Some(Cpv::new(category.clone(), package, version))
}

#[cfg(test)]
mod tests {
    use germ_pms::test_support::cpv;

    use super::*;

    #[test]
    fn test_cpv_from_ebuild_ok() {
        // packge, ebuild, expected package, expected version
        let valid_cases = [
            ("vim", "vim-8.2.3456.ebuild", "vim", "8.2.3456"),
            ("curl", "curl-7.79.1_beta2.ebuild", "curl", "7.79.1_beta2"),
            ("sed", "sed-1-r0.ebuild", "sed", "1-r0"),
            ("zlib", "zlib-1-r01.ebuild", "zlib", "1-r01"),
            ("foo-r2", "foo-r2-2.ebuild", "foo-r2", "2"),
            ("foo-", "foo--1.ebuild", "foo-", "1"),
        ];

        for (package, ebuild, expected_package, expected_version) in valid_cases {
            let parsed = cpv_from_fs_parts(
                &"dev-libs".parse().unwrap(),
                package.parse().unwrap(),
                ebuild,
            )
            .unwrap();

            let cpv = cpv(&format!("dev-libs/{expected_package}-{expected_version}"));
            assert_eq!(parsed, cpv);
        }
    }

    #[test]
    fn test_cpv_from_ebuild_err() {
        // packge, ebuild
        let invalid_cases = [("nano", "vim-1.ebuild"), ("nano", "Manifest")];
        for (package, ebuild) in invalid_cases {
            let parsed = cpv_from_fs_parts(
                &"dev-libs".parse().unwrap(),
                package.parse().unwrap(),
                ebuild,
            );

            assert_eq!(parsed, None);
        }
    }
}
