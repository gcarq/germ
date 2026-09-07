use std::fs;
use std::os::unix;
use std::path::{Path, PathBuf};

use super::portage::PortageConf;
use super::system::SysConf;
use crate::repository::RepoSet;
use crate::repository::test_support::RepoBuilder;

/// Fixture for [`PortageConf`] with one repository profile.
pub struct PortageConfFixture {
    _root: tempfile::TempDir,
    repository: PathBuf,
    sysconf: SysConf,
    repo_set: RepoSet,
}

impl PortageConfFixture {
    pub fn new() -> anyhow::Result<Self> {
        let root = tempfile::tempdir()?;
        let repository = root.path().join("repository");
        RepoBuilder::new("repo")
            .profile("default/linux")
            .profile_file(
                "default/linux/make.defaults",
                "IUSE_IMPLICIT=profile_flag\n",
            )
            .write_to(&repository)?;

        let sysconf = SysConf::new(root.path().to_path_buf());
        let portage = sysconf.portage_conf();
        fs::create_dir_all(&portage)?;
        fs::write(
            portage.join("repos.conf"),
            format!("[repo]\nlocation = {}\n", repository.display()),
        )?;
        fs::write(portage.join("make.conf"), "")?;
        unix::fs::symlink(
            repository.join("profiles/default/linux"),
            portage.join("make.profile"),
        )?;
        let globals = sysconf.default_portage_conf().join("make.globals");
        fs::create_dir_all(globals.parent().expect("make.globals parent"))?;
        fs::write(globals, "ARCH=amd64\n")?;
        Ok(Self {
            _root: root,
            repo_set: RepoSet::new(sysconf.clone().into())?,
            repository,
            sysconf,
        })
    }

    pub fn set_make_globals(&self, contents: &str) -> anyhow::Result<()> {
        let path = self.sysconf.default_portage_conf().join("make.globals");
        Ok(fs::write(path, contents)?)
    }

    pub fn set_profile_make_defaults(&self, contents: &str) -> anyhow::Result<()> {
        let path = self.repository.join("profiles/default/linux/make.defaults");
        Ok(fs::write(path, contents)?)
    }

    pub fn set_make_conf(&self, contents: &str) -> anyhow::Result<()> {
        self.set_portage_file("make.conf", contents)
    }

    pub fn set_portage_file(&self, path: impl AsRef<Path>, contents: &str) -> anyhow::Result<()> {
        let path = self.sysconf.portage_conf().join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(fs::write(path, contents)?)
    }

    pub fn build(&self) -> anyhow::Result<PortageConf> {
        PortageConf::new(&self.repo_set, &self.sysconf)
    }
}
