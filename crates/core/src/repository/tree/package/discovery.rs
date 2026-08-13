use crate::grammar::{PACKAGE, REVISION, VERSION, VERSION_SUFFIXES};
use crate::package::cpv::CPV;
use crate::package::names::{CatName, PkgName};
use crate::package::version::PackageVersion;
use fancy_regex::Regex;
use log::debug;
use std::path::Path;
use std::sync::LazyLock;
use walkdir::WalkDir;

/// Regex to validate and parse `package`, `version`, `suffixes` and the `revision`
/// from an ebuild name.
static EBUILD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"\A(?<package>{PACKAGE})-(?<version>{VERSION})(?<suffixes>{VERSION_SUFFIXES})(?:-r(?<revision>{REVISION}))?\.ebuild\z"
    ))
    .unwrap()
});

/// Resolves all available [`CPV`] on-disk for the given `repo_path` and `category`.
pub fn resolve_cpv_from_category(
    repo_path: &Path,
    category: &CatName,
) -> impl Iterator<Item = anyhow::Result<CPV>> {
    WalkDir::new(repo_path.join(category.as_str()))
        .min_depth(2)
        .max_depth(2)
        .into_iter()
        .filter_entry(|e| {
            e.file_type().is_file()
                && e.file_name()
                    .to_str()
                    .is_some_and(|s| s.ends_with(".ebuild"))
        })
        .filter_map(|entry| {
            let ebuild = entry.ok()?;
            let package = ebuild
                .path()
                .parent()?
                .file_name()?
                .to_str()?
                .parse()
                .ok()?;
            let filename = ebuild.file_name().to_str()?;
            cpv_from_fs_parts(category, package, filename).transpose()
        })
}

/// Parses a `CPV` from the given `category`, `package` and `ebuild`.
///
/// Returns `Ok(None)` if the file is not a valid ebuild or the package name
/// doesn't match the ebuild name.
fn cpv_from_fs_parts(
    category: &CatName,
    package: PkgName,
    ebuild: &str,
) -> anyhow::Result<Option<CPV>> {
    let Some(caps) = EBUILD_RE.captures(ebuild)? else {
        return Ok(None);
    };
    if package.as_str() != &caps["package"] {
        debug!("ebuild is not in the correct directory: {category}/{package}/{ebuild}");
        return Ok(None);
    }

    let revision = caps.name("revision").map(|m| m.as_str());
    match PackageVersion::new(&caps["version"], Some(&caps["suffixes"]), revision) {
        Ok(version) => Ok(Some(CPV::new(category.clone(), package, version))),
        Err(_) => Ok(None),
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
            .unwrap()
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
            )
            .unwrap();

            assert_eq!(parsed, None);
        }
    }
}
