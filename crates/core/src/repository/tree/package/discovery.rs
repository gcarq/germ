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

/// Resolves all available [`CPV`] at the given `repo_path` and `category`.
pub fn resolve_cpv_from_category(
    repo_path: &Path,
    category: &CatName,
) -> impl Iterator<Item = anyhow::Result<CPV>> {
    fs::read_dir(repo_path.join(category.as_str()))
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let pkg_entry = entry.ok()?;
            if !pkg_entry.file_type().ok()?.is_dir() {
                return None;
            }

            let pkg: PkgName = pkg_entry.file_name().to_str()?.parse().ok()?;
            let entries = fs::read_dir(pkg_entry.path()).ok()?;
            Some((pkg, entries))
        })
        .flat_map(move |(package, ebuilds)| {
            ebuilds.filter_map(move |entry| {
                let entry = entry.ok()?;
                if !entry.file_type().ok()?.is_file() {
                    return None;
                }

                let ebuild = entry.file_name().into_string().ok()?;
                cpv_from_fs_parts(category, package.clone(), &ebuild).transpose()
            })
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
    let Some(stem) = ebuild.strip_suffix(".ebuild") else {
        return Ok(None);
    };
    let Some(version) = stem
        .strip_prefix(package.as_str())
        .and_then(|rem| rem.strip_prefix('-'))
    else {
        debug!("ebuild is not in the correct directory: {category}/{package}/{ebuild}");
        return Ok(None);
    };

    let Some(caps) = VERSION_RE.captures(version)? else {
        return Ok(None);
    };
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
