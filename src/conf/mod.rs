pub mod repos;

use crate::conf::repos::ReposConf;
use crate::r#const::DEFAULT_PORTAGE_CONF_PATH;
use crate::makenv::MakeEnv;
use crate::profile::{InheritFrom, Profile};
use crate::repository::Repository;
use crate::utils::FileFromPath;
use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::path::Path;

/// Holds the portage configuration that usually resides in /etc/portage.
#[derive(Debug)]
pub struct PortageConf {
    pub make_env: MakeEnv,
    pub repos: ReposConf,
    pub use_manager: UseManager,
    profile: Profile,
}

impl PortageConf {
    /// Builds a [`PortageConf`] from the given portage configuration path.
    pub fn new(path: &Path) -> Result<Self> {
        let repos = ReposConf::new(&path.join("repos.conf"))
            .with_context(|| "Unable to process repos.conf")?;

        let profile_path = path.join("make.profile");
        let profile = Profile::new(&profile_path, &repos)
            .with_context(|| "Unable to build profile from make.profile")?;

        let make_env = Self::init_make_env(path, &profile)?;

        let arch = make_env
            .get("ARCH")
            .with_context(|| "Missing ARCH variable")?
            .to_string();
        Self::validate_arch(&arch, &repos.repositories())?;
        Self::validate_profile(&profile_path, &arch, &repos.repositories())?;

        Ok(PortageConf {
            use_manager: UseManager::new(&profile, &repos.repositories()),
            make_env,
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
            let profile_prefix = format!("{}/profiles/", repo.path.canonicalize()?.display());
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

#[derive(Debug)]
pub struct UseManager {
    // Holds the USE masks for each repository
    repo_use_mask: HashMap<String, Vec<String>>,
}

impl UseManager {
    pub fn new(profile: &Profile, repos: &[&Repository]) -> Self {
        let use_mask = repos
            .iter()
            .map(|repo| (repo.name.clone(), repo.package_mask.to_vec()))
            .collect();
        Self {
            repo_use_mask: use_mask,
        }
    }
}
