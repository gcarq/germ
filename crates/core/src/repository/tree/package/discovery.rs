use crate::package::cpv::CPV;
use crate::package::version::PackageVersion;
use crate::regex::PV_REV;
use anyhow::{Context, anyhow};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;
use walkdir::WalkDir;

/// Regex to validate and parse `package`, `version`, `suffixes` and the `revision`
/// from an ebuild name.
static EBUILD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^{PV_REV}.ebuild$")).unwrap());

/// Resolves all available [`CPV`] on-disk for the given `repo_path` and `category`.
pub fn resolve_cpv_from_category(
    repo_path: &Path,
    category: &str,
) -> impl Iterator<Item = anyhow::Result<CPV>> {
    WalkDir::new(repo_path.join(category))
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
            let package = ebuild.path().parent()?.file_name()?.to_str()?;
            let filename = ebuild.file_name().to_str()?;
            cpv_from_fs_parts(category, package, filename).transpose()
        })
}

/// Parses a `CPV` from the given `category`, `package` and `ebuild`.
///
/// Returns `Ok(None)` if the file is not a valid ebuild or the package name
/// doesn't match the ebuild name.
/// Returns `Err` if the file is a valid regex, but no valid [`CPV`].
fn cpv_from_fs_parts(category: &str, package: &str, ebuild: &str) -> anyhow::Result<Option<CPV>> {
    let Some(caps) = EBUILD_RE.captures(ebuild) else {
        return Ok(None);
    };
    if package != &caps["package"] {
        return Ok(None);
    }
    let version = PackageVersion::new(
        &caps["version"],
        Some(&caps["suffixes"]),
        caps.name("revision").map(|m| m.as_str()),
    )
    .with_context(|| anyhow!("unable to parse version from ebuild: '{ebuild}'"))?;
    Ok(Some(CPV::new_unchecked(category, package, version)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpv_from_ebuild_path_ok() {
        // (category, file name, is_some)
        let valid_ebuilds = [
            ("acct-user", "err", "err-0-r2.ebuild", true),
            ("acct-user", "err-0-r2", "err-0-r2", false),
            ("app-editors", "vim", "vim-8.2.3456.ebuild", true),
            ("app-editors", "vim", "vim-8.2.3456-r0.ebuild", true),
            ("app-editors", "vim", "vim-8.2.3456-r1.ebuild", true),
            ("dev-lang", "rust", "rust-1.65.0_alpha1-r2.ebuild", true),
            ("net-misc", "curl", "curl-7.79.1_beta2.ebuild", true),
            ("net-misc", "curl", "Manifest", false),
        ];
        for (category, package, ebuild, is_some) in valid_ebuilds {
            let cpv = cpv_from_fs_parts(category, package, ebuild);
            assert!(cpv.is_ok(), "CPV from '{ebuild}' should be valid");
            assert_eq!(
                cpv.is_ok_and(|cpv| cpv.is_some()),
                is_some,
                "failure for {category}/{package}/{ebuild}",
            );
        }
    }

    #[test]
    fn test_cpv_from_explicit_r0_ebuild() {
        let cpv = cpv_from_fs_parts("dev-libs", "pkg", "pkg-1.0-r0.ebuild")
            .unwrap()
            .unwrap();

        assert_eq!(cpv.fqn(), "dev-libs/pkg-1.0-r0");
        assert_eq!(cpv.pf(), "pkg-1.0-r0");
    }

    #[test]
    fn test_cpv_from_revision_source() {
        let cpv = cpv_from_fs_parts("dev-libs", "example", "example-1.0.0-r0101.ebuild")
            .unwrap()
            .unwrap();

        assert_eq!(cpv.fqn(), "dev-libs/example-1.0.0-r0101");
        assert_eq!(cpv.pf(), "example-1.0.0-r0101");
    }
}
