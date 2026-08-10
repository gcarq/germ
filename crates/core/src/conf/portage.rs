use std::path::Path;

use crate::SysConf;
use crate::conf::masks::useflag::UseMasks;
use crate::conf::masks::{PackageMasks, UserPackageMasks};
use crate::files::{UseEntries, entry::Precedence, pkguse::PackageUseEntries};
use crate::makenv::MakeEnv;
use crate::profile::Profile;
use crate::repository::RepoSet;
use crate::utils::Inherit;
use anyhow::{Context, anyhow};
use log::debug;

/// Holds the portage configuration that usually resides in `/etc/portage`.
pub struct PortageConf {
    pub make_env: MakeEnv,
    pub package_masks: PackageMasks,
    pub use_masks: UseMasks,
}

impl PortageConf {
    /// Builds a [`PortageConf`] from the given `repo_set` and `sysconf`.
    pub fn new(repo_set: &RepoSet, sysconf: &SysConf) -> anyhow::Result<Self> {
        let path = sysconf.portage_conf();
        let profile_path = path.join("make.profile");
        debug!(
            "Configured profile {}",
            profile_path.canonicalize()?.display()
        );
        let profile = repo_set
            .resolve_profile(&profile_path)
            .with_context(|| anyhow!("unable to build profile from {}", profile_path.display()))?;

        let make_env = Self::init_make_env(&profile, &path)?;

        let arch = make_env
            .get("ARCH")
            .with_context(|| "missing ARCH variable")?
            .to_string();
        repo_set.validate_arch(&arch)?;
        repo_set.validate_profile(&profile, &arch)?;

        let package_masks = PackageMasks::new(
            repo_set.package_masks(),
            &profile,
            UserPackageMasks::from_path(&path)?,
        );

        let use_masks = UseMasks::new(
            &profile,
            PackageUseEntries::from_path(&path.join("package.use"), Precedence::User, true)?,
            UseEntries::from_path(
                &path.join("profile").join("use.mask"),
                Precedence::User,
                true,
            )?,
            PackageUseEntries::from_path(
                &path.join("profile").join("package.use.mask"),
                Precedence::User,
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
    fn init_make_env(profile: &Profile, path: &Path) -> anyhow::Result<MakeEnv> {
        let globals_path = path.join("make.globals");
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
}
