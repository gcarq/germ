pub mod repos;

use crate::conf::repos::ReposConf;
use crate::makenv::MakeEnv;
use crate::profile::{InheritFrom, Profile};
use crate::utils::FileFromPath;
use anyhow::{Context, Result};
use std::path::Path;

/// Holds the portage configuration that usually resides in /etc/portage.
#[derive(Debug)]
pub struct PortageConf {
    pub make_env: MakeEnv,
    pub repos: ReposConf,
    profile: Profile,
}

impl PortageConf {
    /// Builds a [`PortageConf`] from the given portage configuration path.
    pub fn new(path: &Path) -> Result<Self> {
        let repos = ReposConf::new(&path.join("repos.conf"))
            .with_context(|| "Unable to process repos.conf")?;
        let mut make_env = MakeEnv::from_path(&path.join("make.conf"), true, false)
            .with_context(|| "Unable to process make.conf")?;
        let profile = Profile::new(&path.join("make.profile"), &repos)
            .with_context(|| "Unable to build profile from make.profile")?;

        make_env.inherit_from(&profile.make_defaults);

        Ok(PortageConf {
            make_env,
            profile,
            repos,
        })
    }
}
