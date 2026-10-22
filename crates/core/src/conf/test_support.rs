use std::os::unix::fs::symlink;

use anyhow::Context;
use tempfile::TempDir;

use super::portage::PortageConf;
use crate::repository::test_support::{RepoBuilder, repo_set_in};
use crate::test_support::{build_sysconf, write_file};

/// The repository used by package-set fixtures.
const TEST_REPO: &str = "repo";
/// The profile used by package-set fixtures.
const TEST_PROFILE: &str = "default/linux";
/// The `packages` file used by package-set fixtures.
const TEST_PACKAGES: &str = "*sys-libs/glibc\napp-misc/profile-tool\n";

/// Builds a [`PortageConf`] rooted in the given `temp_dir`.
pub fn build_portage_conf(temp_dir: &TempDir) -> anyhow::Result<PortageConf> {
    let repo = RepoBuilder::new(TEST_REPO)
        .formats(["portage-2"])
        .profile(TEST_PROFILE)
        .profile_file(format!("{TEST_PROFILE}/packages"), TEST_PACKAGES);

    let sysconf = build_sysconf(temp_dir);
    let reposet = repo_set_in(temp_dir, [repo])?;
    let location = reposet
        .iter()
        .next()
        .context("no repository configured")?
        .location();

    write_file(sysconf.default_portage_conf().join("make.globals"), "")?;
    write_file(sysconf.portage_conf().join("make.conf"), "ARCH=\"amd64\"\n")?;

    let profile_path = location.join("profiles").join(TEST_PROFILE);
    let link = sysconf.portage_conf().join("make.profile");
    symlink(profile_path, &link).with_context(|| format!("failed to link {}", link.display()))?;

    PortageConf::new(&reposet, &sysconf)
}
