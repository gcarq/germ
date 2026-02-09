mod manager;
pub mod repos;

use crate::conf::repos::ReposConf;
use crate::consts::DEFAULT_PORTAGE_CONF_PATH;
use crate::linefile::LineBasedFile;
use crate::makenv::MakeEnv;
use crate::profile::Profile;
use crate::repository::Repository;
use crate::utils::{FileFromPath, Inherit};
use anyhow::{Context, Result, anyhow};
use manager::MaskManager;
use std::path::Path;

/// Holds the portage configuration that usually resides in /etc/portage.
pub struct PortageConf {
    pub make_env: MakeEnv,
    pub repos_conf: ReposConf,
    profile: Profile,
    pub mask_manager: MaskManager,
}

impl PortageConf {
    /// Builds a [`PortageConf`] from the given portage configuration path.
    pub fn new(path: &Path) -> Result<Self> {
        let repos_conf = ReposConf::new(&path.join("repos.conf"))
            .with_context(|| "Unable to process repos.conf")?;

        let profile_path = path.join("make.profile");
        let profile = Profile::new(&profile_path, &repos_conf)
            .with_context(|| "Unable to build profile from make.profile")?;

        let make_env = Self::init_make_env(path, &profile)?;

        let repos = repos_conf.repositories().collect::<Vec<_>>();
        let arch = make_env
            .get("ARCH")
            .with_context(|| "Missing ARCH variable")?
            .to_string();
        Self::validate_arch(&arch, &repos)?;
        Self::validate_profile(&profile_path, &arch, &repos)?;

        let mask_manager = MaskManager::new(
            &repos,
            &profile,
            LineBasedFile::from_path(&path.join("package.mask"), true, true)?,
            LineBasedFile::from_path(&path.join("package.unmask"), true, true)?,
        )
        .with_context(|| "Unable to build MaskManager")?;

        Ok(PortageConf {
            make_env,
            repos_conf,
            profile,
            mask_manager,
        })
    }

    /// Initializes and returns the make environment by processing make.globals,
    /// make.defaults from profile and make.conf (in this order).
    fn init_make_env(path: &Path, profile: &Profile) -> Result<MakeEnv> {
        let globals_path = Path::new(DEFAULT_PORTAGE_CONF_PATH).join("make.globals");
        let make_globals = MakeEnv::from_path(&globals_path, true, false)
            .with_context(|| "Unable to process make.globals")?;
        // TODO: make.conf: Variables prefixed with __ are local should not be propagated.
        let make_conf = MakeEnv::from_path(&path.join("make.conf"), true, false)
            .with_context(|| "Unable to process make.conf")?;

        let mut make_env = MakeEnv::default();
        make_env.inherit_from(&make_globals);
        make_env.inherit_from(&profile.make_defaults);
        make_env.inherit_from(&make_conf);
        Ok(make_env)
    }

    /// Sanity check to ensure the given ARCH is supported by at least one repository.
    fn validate_arch(arch: &str, repos: &[&Repository]) -> Result<()> {
        match repos.iter().any(|repo| repo.arch_list.contains(arch)) {
            true => Ok(()),
            false => Err(anyhow!(
                "ARCH value '{arch}' is not supported by any configured repository"
            )),
        }
    }

    /// Sanity check to ensure the given profile is valid for at least one repository.
    fn validate_profile(path: &Path, arch: &str, repos: &[&Repository]) -> Result<()> {
        let profile_path = path.canonicalize()?.display().to_string();
        for repo in repos {
            let profile_prefix = format!("{}/profiles/", repo.location.canonicalize()?.display());
            if let Some(p) = profile_path.strip_prefix(&profile_prefix)
                && repo.is_known_profile(arch, p)
            {
                return Ok(());
            }
        }
        Err(anyhow!(
            "Profile at {profile_path} is not valid for any configured repository"
        ))
    }
}
