use crate::package::cpv::CPV;
use crate::package::version::PackageVersion;
use crate::repository::EBUILD_RE;
use anyhow::{Context, Result, anyhow};
use std::path::Path;
use walkdir::WalkDir;

/// Resolves all available [`CPV`] on-disk for the given `repo_path` and `category`.
pub fn resolve_cpv_from_category(
    repo_path: &Path,
    category: &str,
) -> impl Iterator<Item = Result<CPV>> {
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
        .filter_map(Result::ok)
        .filter_map(|e| {
            e.file_name()
                .to_str()
                .and_then(|filename| cpv_from_ebuild_name(category, filename).transpose())
        })
}

/// Parses a `CPV` from the given `category` and ebuild `filename`.
///
/// Returns `Ok(None)` if the file is not a valid ebuild.
/// Returns `Err` if the file is a valid regex, but doesn't belong here.
fn cpv_from_ebuild_name(category: &str, filename: &str) -> Result<Option<CPV>> {
    let Some(caps) = EBUILD_RE.captures(filename) else {
        return Ok(None);
    };
    let package = &caps["package"];
    let version = PackageVersion::new(
        &caps["version"],
        Some(&caps["suffixes"]),
        caps.name("revision").map(|m| m.as_str()),
    )
    .with_context(|| anyhow!("unable to parse version from ebuild: '{filename}'"))?;
    Ok(Some(CPV::new_unchecked(category, package, version)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpv_from_ebuild_path_ok() {
        // (category, file name, is_some)
        let valid_ebuilds = [
            ("app-editors", "vim-8.2.3456.ebuild", true),
            ("app-editors", "vim-8.2.3456-r0.ebuild", true),
            ("app-editors", "vim-8.2.3456-r1.ebuild", true),
            ("dev-lang", "rust-1.65.0_alpha1-r2.ebuild", true),
            ("net-misc", "curl-7.79.1_beta2_p20220101.ebuild", true),
            ("net-misc", "Manifest", false),
        ];
        for (category, filename, is_some) in valid_ebuilds {
            let cpv = cpv_from_ebuild_name(category, filename);
            assert!(cpv.is_ok(), "CPV from '{filename}' should be valid");
            assert_eq!(
                cpv.unwrap().is_some(),
                is_some,
                "failure for ebuild '{filename}'",
            );
        }
    }

    #[test]
    fn test_cpv_from_explicit_r0_ebuild() {
        let cpv = cpv_from_ebuild_name("dev-libs", "pkg-1.0-r0.ebuild")
            .unwrap()
            .unwrap();

        assert_eq!(cpv.fqn(), "dev-libs/pkg-1.0");
        assert_eq!(cpv.pf(), "pkg-1.0-r0");
    }

    #[test]
    fn cpv_from_ebuild_path_none() {
        // (category, ebuild path)
        let invalid_ebuilds = [
            ("app-editors", "vim8.2.3456.ebuild"),
            ("app-editors", "vim-.ebuild"),
            ("dev-lang", "rust-1.65.0_alphaX-r2.ebuild"),
            ("net-misc", "curl-7.79.1--r1.ebuild"),
            ("net-misc", "curl-7.79.1_beta2_p20220101-rX.ebuild"),
        ];
        for (category, filename) in invalid_ebuilds {
            let cpv = cpv_from_ebuild_name(category, filename);
            assert!(cpv.is_ok(), "result from '{filename}' should be ok");
            assert!(
                cpv.unwrap().is_none(),
                "CPV from '{filename}' should be None"
            );
        }
    }
}
