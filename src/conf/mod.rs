pub mod repos;

use crate::conf::repos::ReposConf;
use crate::r#const::DEFAULT_PORTAGE_CONF_PATH;
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
        let profile = Profile::new(&path.join("make.profile"), &repos)
            .with_context(|| "Unable to build profile from make.profile")?;

        Ok(PortageConf {
            make_env: Self::init_make_env(path, &profile)?,
            repos,
            profile,
        })
    }

    /// Initializes and returns the make environment by processing make.globals,
    /// make.defaults from profile and make.conf (in this order).
    fn init_make_env(path: &Path, profile: &Profile) -> Result<MakeEnv> {
        let globals_path = Path::new(DEFAULT_PORTAGE_CONF_PATH).join("make.globals");
        let make_globals = MakeEnv::from_path(&globals_path, true, false)
            .with_context(|| "Unable to process make.globals")?;
        // TODO: Treat variables with __ prefix as local
        let mut make_conf = MakeEnv::from_path(&path.join("make.conf"), true, false)
            .with_context(|| "Unable to process make.conf")?;
        make_conf.inherit_from(&make_globals);
        make_conf.inherit_from(&profile.make_defaults);
        Ok(make_conf)
    }
}
