mod masks;

use crate::conf::masks::useflag::UseMasks;
use crate::consts::DEFAULT_PORTAGE_CONF_PATH;
use crate::files::pkguse::PackageUseEntries;
use crate::files::{FileFromPath, PackageEntries, UseEntries};
use crate::makenv::MakeEnv;
use crate::profile::Profile;
use crate::repository::set::RepoSet;
use crate::utils::Inherit;
use anyhow::{Context, Result, anyhow};
use log::debug;
use masks::PackageMasks;
use std::path::Path;

/// Holds the portage configuration that usually resides in `/etc/portage`.
pub struct PortageConf {
    pub make_env: MakeEnv,
    pub package_masks: PackageMasks,
    pub use_masks: UseMasks,
}

impl PortageConf {
    /// Builds a [`PortageConf`] from the given portage configuration `path` and `repo_set`.
    pub fn new(path: &Path, repo_set: &RepoSet) -> Result<Self> {
        let profile_path = path.join("make.profile");
        debug!(
            "Configured profile {}",
            profile_path.canonicalize()?.display()
        );
        let profile = Profile::resolve(&profile_path, repo_set)
            .with_context(|| anyhow!("unable to build profile from {}", profile_path.display()))?;

        let make_env = Self::init_make_env(path, &profile)?;

        let arch = make_env
            .get("ARCH")
            .with_context(|| "missing ARCH variable")?
            .to_string();
        Self::validate_arch(&arch, repo_set)?;
        Self::validate_profile(&profile, &arch, repo_set)?;

        let package_masks = PackageMasks::new(
            repo_set,
            &profile,
            PackageEntries::from_path(&path.join("package.mask"), true, true)?,
            PackageEntries::from_path(&path.join("package.unmask"), true, true)?,
        );

        let use_masks = UseMasks::new(
            &profile,
            PackageUseEntries::from_path(&path.join("package.use"), true, true)?,
            UseEntries::from_path(&path.join("profile").join("use.mask"), true, true)?,
            PackageUseEntries::from_path(
                &path.join("profile").join("package.use.mask"),
                true,
                true,
            )?,
        );

        Ok(PortageConf {
            make_env,
            package_masks,
            use_masks,
        })
    }

    /// Initializes and returns the make environment by processing make.globals,
    /// make.defaults from profile and make.conf (in this order).
    fn init_make_env(path: &Path, profile: &Profile) -> Result<MakeEnv> {
        let globals_path = Path::new(DEFAULT_PORTAGE_CONF_PATH).join("make.globals");
        let make_globals = MakeEnv::from_path(&globals_path, true, false)
            .with_context(|| "unable to process make.globals")?;
        let make_conf = MakeEnv::from_path(&path.join("make.conf"), true, false)
            .with_context(|| "unable to process make.conf")?;

        let env = MakeEnv::default()
            .inherit(&make_globals)
            .inherit(&profile.make_defaults)
            .inherit(&make_conf);
        Ok(env)
    }

    /// Sanity check to ensure the given `ARCH` is supported by at least one repository.
    fn validate_arch(arch: &str, repo_set: &RepoSet) -> Result<()> {
        match repo_set.values().any(|repo| repo.arch_list.supports(arch)) {
            true => Ok(()),
            false => Err(anyhow!(
                "ARCH value '{arch}' is not supported by any configured repository"
            )),
        }
    }

    /// Sanity check to ensure the given `profile` is valid for at least one repository.
    ///
    /// The profile- and the repository location are expected to be absolute paths.
    fn validate_profile(profile: &Profile, arch: &str, repo_set: &RepoSet) -> Result<()> {
        for repo in repo_set.values() {
            let profile_prefix = format!("{}/profiles/", repo.location.display());
            if let Some(p) = profile
                .location
                .display()
                .to_string()
                .strip_prefix(&profile_prefix)
                && repo.is_known_profile(arch, p)
            {
                return Ok(());
            }
        }
        Err(anyhow!(
            "Profile {profile} is not valid for any configured repository"
        ))
    }
}
