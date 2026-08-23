use crate::grammar::{REVISION, VERSION, VERSION_SUFFIXES};
use crate::package::cpv::CPV;
use crate::package::names::{CatName, PkgName};
use crate::package::version::PackageVersion;
use fancy_regex::Regex;
use log::debug;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

/// Regex to validate and parse `version`, `suffixes` and the `revision` from an ebuild file name.
static VERSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"\A(?<version>{VERSION})(?<suffixes>{VERSION_SUFFIXES})(?:-r(?<revision>{REVISION}))?\z"
    ))
    .unwrap()
});

/// Resolves all [`CPV`] from the given repository path and categories.
pub fn resolve_from_repo_path<'r>(
    repo_path: &'r Path,
    categories: impl IntoIterator<Item = &'r CatName>,
) -> impl Iterator<Item = CPV> {
    categories
        .into_iter()
        .flat_map(|cat| resolve_from_category_path(cat, &repo_path.join(cat.as_str())))
}

/// Resolves all [`CPV`] from the given category path.
pub fn resolve_from_category_path<'c>(
    category: &'c CatName,
    cat_path: &Path,
) -> impl Iterator<Item = CPV> + use<'c> {
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

/// Resolves all [`CPV`] from the given package path.
pub fn resolve_from_pkg_path<'c>(
    category: &'c CatName,
    package: PkgName,
    pkg_path: &Path,
) -> impl Iterator<Item = CPV> + use<'c> {
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

/// Parses a [`CPV`] from the given `category`, `package` and `ebuild_filename`.
///
/// Returns `None` if the file is not a valid ebuild or the package name doesn't match the ebuild name.
///
/// # Panics
///
/// Will panic if the regex engine fails e.g. backtracking limit is exceeded.
fn cpv_from_fs_parts(category: &CatName, package: PkgName, ebuild_filename: &str) -> Option<CPV> {
    let Some(version) = ebuild_filename
        .strip_suffix(".ebuild")?
        .strip_prefix(package.as_str())
        .and_then(|rem| rem.strip_prefix('-'))
    else {
        debug!("ebuild is not in the correct directory: {category}/{package}/{ebuild_filename}");
        return None;
    };

    let caps = match VERSION_RE.captures(version) {
        Ok(caps) => caps?,
        Err(err) => {
            panic!(
                "BUG: regex engine error while parsing version from ebuild filename {ebuild_filename}: {err}"
            );
        }
    };

    let revision = caps.name("revision").map(|m| m.as_str());
    match PackageVersion::new(&caps["version"], Some(&caps["suffixes"]), revision) {
        Ok(version) => Some(CPV::new(category.clone(), package, version)),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::cpv;

    #[test]
    fn test_cpv_from_ebuild_ok() {
        // packge, ebuild, expected package, expected version
        let valid_cases = [
            ("vim", "vim-8.2.3456.ebuild", "vim", "8.2.3456"),
            ("curl", "curl-7.79.1_beta2.ebuild", "curl", "7.79.1_beta2"),
            ("pkg", "pkg-1.0-r0.ebuild", "pkg", "1.0-r0"),
            (
                "example",
                "example-1.0.0-r0101.ebuild",
                "example",
                "1.0.0-r0101",
            ),
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

            assert_eq!(parsed, cpv("dev-libs", expected_package, expected_version));
        }
    }

    #[test]
    fn test_cpv_from_ebuild_err() {
        // packge, ebuild
        let invalid_cases = [("foo", "bar-1.ebuild"), ("foo", "Manifest")];
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
