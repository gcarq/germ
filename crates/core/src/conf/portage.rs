use std::path::PathBuf;

use crate::SysConf;
use crate::files::entry::Precedence;
use crate::files::pkgfile::{PackageAcceptKeywords, PackageUseRecords};
use crate::files::{PackageEntries, UseEntries};
use crate::makenv::{MakeEnv, MakeEnvStack};
use crate::policy::keyword::EffectiveKeywords;
use crate::policy::pkgmask::PortageSource as PackageMaskSource;
use crate::policy::useflag::{LocalRecords as UseLocalRecords, UsePolicy};
use crate::profile::Profile;
use crate::repository::RepoSet;
use crate::utils::Inherit;
use anyhow::Context;
use log::debug;

/// Responsible for loading and validation an existing portage configuration,
/// usually located at `/etc/portage`.
pub struct PortageConf {
    path: PathBuf,
    profile: Profile,
    makenv_stack: MakeEnvStack,
}

impl PortageConf {
    /// Builds a [`PortageConf`] from the given `reposet` and `sysconf`.
    pub fn new(reposet: &RepoSet, sysconf: &SysConf) -> anyhow::Result<Self> {
        let path = sysconf.portage_conf();
        let profile_path = path.join("make.profile");
        debug!(
            "Configured profile {}",
            profile_path.canonicalize()?.display()
        );
        let profile = reposet
            .resolve_profile(&profile_path)
            .with_context(|| format!("unable to build profile from {}", profile_path.display()))?;

        let makenv_stack = Self::init_makenv(&profile, sysconf)?;
        let arch = makenv_stack.arch()?;
        reposet.validate_arch(&arch)?;
        reposet.validate_profile(&profile, &arch)?;

        Ok(Self {
            path,
            profile,
            makenv_stack,
        })
    }

    /// Returns the resolved [`MakeEnv`].
    pub const fn makenv(&self) -> &MakeEnv {
        self.makenv_stack.makenv()
    }

    /// Returns the [`EffectiveKeywords`] from profile and user config.
    pub fn effective_keywords(&self) -> anyhow::Result<EffectiveKeywords> {
        let local = PackageAcceptKeywords::from_path(
            &self.path.join("package.accept_keywords"),
            Precedence::User,
            true,
        )?;
        Ok(EffectiveKeywords::new(
            self.makenv_stack
                .accept_keywords()
                .context("unable to parse ACCEPT_KEYWORDS")?,
            local.inherit(&self.profile.package_accept_keywords)?,
        ))
    }

    /// Returns a [`PackageMaskSource`] from profile and user config.
    pub fn package_mask_source(&self) -> anyhow::Result<PackageMaskSource> {
        Ok(PackageMaskSource {
            profile_mask: self.profile.package_mask.clone(),
            profile_unmask: self.profile.package_unmask.clone(),
            local_mask: PackageEntries::from_path(
                &self.path.join("package.mask"),
                Precedence::User,
                true,
            )?,
            local_unmask: PackageEntries::from_path(
                &self.path.join("package.unmask"),
                Precedence::User,
                true,
            )?,
        })
    }

    /// Returns [`UsePolicy`] from profile and user config.
    pub fn use_policy(&self) -> anyhow::Result<UsePolicy> {
        UsePolicy::new(
            self.makenv_stack.global_use()?,
            self.makenv_stack.iuse_implicit()?,
            self.profile.use_records.clone(),
            UseLocalRecords {
                package_use: PackageUseRecords::from_path(
                    &self.path.join("package.use"),
                    Precedence::User,
                    true,
                )?,
                use_mask: UseEntries::from_path(
                    &self.path.join("profile").join("use.mask"),
                    Precedence::User,
                    true,
                )?,
                package_use_mask: PackageUseRecords::from_path(
                    &self.path.join("profile").join("package.use.mask"),
                    Precedence::User,
                    true,
                )?,
            },
        )
    }

    /// Initializes the make env and takes care of inheritance.
    ///
    /// The order of processing is:
    ///   * make.globals
    ///   * make_defaults from active profile
    ///   * make.conf
    fn init_makenv(profile: &Profile, sysconf: &SysConf) -> anyhow::Result<MakeEnvStack> {
        let global = MakeEnv::from_path(
            &sysconf.default_portage_conf().join("make.globals"),
            true,
            false,
        )
        .context("unable to process make.globals")?;
        let user = MakeEnv::from_path(&sysconf.portage_conf().join("make.conf"), true, false)
            .context("unable to process make.conf")?;
        MakeEnvStack::new(global, profile.make_defaults.clone(), user)
            .context("unable to resolve make environment")
    }
}
