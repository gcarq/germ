use std::path::PathBuf;
use std::str::FromStr;

use crate::SysConf;
use crate::files::entry::Precedence;
use crate::files::pkgfile::{PackageAcceptKeywords, PackageUseRecords};
use crate::files::{PackageEntries, UseEntries};
use crate::makenv::{IncrementalVars, MakeEnv};
use crate::policy::keyword::EffectiveKeywords;
use crate::policy::pkgmask::PortageSource as PackageMaskSource;
use crate::policy::useflag::{
    LocalRecords as UseLocalRecords, ProfileRecords as UseProfileRecords, UsePolicy,
};
use crate::profile::Profile;
use crate::repository::RepoSet;
use crate::types::FxHashSet;
use crate::useflag::{UseExpandConfig, UseFlag};
use crate::utils::Inherit;
use anyhow::Context;
use log::debug;

/// Responsible for loading and validation an existing portage configuration,
/// usually located at `/etc/portage`.
pub struct PortageConf {
    path: PathBuf,
    profile: Profile,
    makenv: MakeEnv,
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
            .with_context(|| format!("unable to build profile from {}", profile_path.display()))?;

        let makenv = Self::init_makenv(&profile, sysconf)?;
        let arch = makenv
            .get("ARCH")
            .context("missing ARCH variable")?
            .to_string()
            .parse()?;
        repo_set.validate_arch(&arch)?;
        repo_set.validate_profile(&profile, &arch)?;

        Ok(Self {
            path,
            profile,
            makenv,
        })
    }

    /// Returns the resolved [`MakeEnv`].
    pub const fn makenv(&self) -> &MakeEnv {
        &self.makenv
    }

    /// Returns the [`EffectiveKeywords`] from profile and user config.
    pub fn effective_keywords(&self) -> anyhow::Result<EffectiveKeywords> {
        let accept_keywords = self.makenv.get("ACCEPT_KEYWORDS").map(ToString::to_string);
        let local = PackageAcceptKeywords::from_path(
            &self.path.join("package.accept_keywords"),
            Precedence::User,
            true,
        )?;
        EffectiveKeywords::new(
            accept_keywords.as_deref(),
            local.inherit(&self.profile.package_accept_keywords)?,
        )
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
            self.makenv(),
            self.iuse_implicit()?,
            UseProfileRecords {
                make_defaults: self.profile.make_defaults.clone(),
                package_use: self.profile.package_use.clone(),
                package_use_mask: self.profile.package_use_mask.clone(),
                package_use_force: self.profile.package_use_force.clone(),
                package_use_stable_mask: self.profile.package_use_stable_mask.clone(),
                package_use_stable_force: self.profile.package_use_stable_force.clone(),
                use_mask: self.profile.use_mask.clone(),
                use_force: self.profile.use_force.clone(),
                use_stable_mask: self.profile.use_stable_mask.clone(),
                use_stable_force: self.profile.use_stable_force.clone(),
            },
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
    fn init_makenv(profile: &Profile, sysconf: &SysConf) -> anyhow::Result<MakeEnv> {
        let globals_path = sysconf.default_portage_conf().join("make.globals");
        let make_globals = MakeEnv::from_path(&globals_path, true, false)
            .context("unable to process make.globals")?;
        let make_conf = MakeEnv::from_path(&sysconf.portage_conf().join("make.conf"), true, false)
            .context("unable to process make.conf")?;

        let layers = [&make_globals, &profile.make_defaults, &make_conf];
        let provisional = MakeEnv::fold(&layers, &IncrementalVars::default())?;
        let expand =
            UseExpandConfig::from_makenv(&provisional).context("invalid USE expand config")?;
        let vars = IncrementalVars::from(expand.names());
        MakeEnv::fold(&layers, &vars)
    }

    /// Returns the set `IUSE_IMPLICIT` from [`MakeEnv`].
    fn iuse_implicit(&self) -> anyhow::Result<FxHashSet<UseFlag>> {
        match self.profile.make_defaults.get("IUSE_IMPLICIT") {
            Some(value) => value
                .to_string()
                .split_whitespace()
                .map(UseFlag::from_str)
                .collect::<Result<_, _>>()
                .context("unable to parse IUSE_IMPLICIT"),
            None => Ok(FxHashSet::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::PortageConfFixture;
    use super::*;

    fn configure_expand(fixture: &PortageConfFixture) -> anyhow::Result<()> {
        fixture.set_profile_make_defaults(
            "IUSE_IMPLICIT=profile_flag
            USE_EXPAND=VIDEO_CARDS
            VIDEO_CARDS=nouveau
            ",
        )?;
        fixture.set_make_globals(
            "ARCH=amd64
            USE_EXPAND=VIDEO_CARDS
            VIDEO_CARDS=amdgpu
            ",
        )
    }

    #[test]
    fn test_portage_conf_expand_member_inherit() -> anyhow::Result<()> {
        let fixture = PortageConfFixture::new()?;
        configure_expand(&fixture)?;
        fixture.set_make_conf("VIDEO_CARDS=\"-amdgpu radeonsi\"\n")?;
        let conf = fixture.build()?;

        assert_eq!(
            conf.makenv().get("VIDEO_CARDS").map(ToString::to_string),
            Some("nouveau radeonsi".into())
        );
        Ok(())
    }

    #[test]
    fn test_portage_conf_expand_groups() -> anyhow::Result<()> {
        let fixture = PortageConfFixture::new()?;
        configure_expand(&fixture)?;
        fixture.set_make_conf(
            "USE_EXPAND=\"-VIDEO_CARDS INPUT_DEVICES\"
            INPUT_DEVICES=libinput
            ",
        )?;
        let conf = fixture.build()?;
        let expand = UseExpandConfig::from_makenv(conf.makenv())?;

        assert_eq!(
            expand.materialize(conf.makenv())?,
            vec![UseFlag::new("input_devices_libinput")?]
        );
        Ok(())
    }

    #[test]
    fn test_portage_conf_expand_overlap() -> anyhow::Result<()> {
        let fixture = PortageConfFixture::new()?;
        configure_expand(&fixture)?;
        fixture.set_make_conf("USE_EXPAND_UNPREFIXED=VIDEO_CARDS\n")?;

        assert!(fixture.build().is_err());
        Ok(())
    }

    #[test]
    fn test_iuse_implicit_ignores_make_conf() -> anyhow::Result<()> {
        let fixture = PortageConfFixture::new()?;
        fixture.set_make_conf("IUSE_IMPLICIT=local_flag\n")?;
        let conf = fixture.build()?;

        assert_eq!(
            conf.iuse_implicit()?,
            FxHashSet::from_iter([UseFlag::new("profile_flag")?])
        );
        Ok(())
    }

    #[test]
    fn test_portage_conf_skips_keyword_policy() -> anyhow::Result<()> {
        let fixture = PortageConfFixture::new()?;
        fixture.set_portage_file("package.accept_keywords", "invalid atom")?;
        let conf = fixture.build()?;

        assert!(conf.use_policy().is_ok());
        assert!(conf.effective_keywords().is_err());
        Ok(())
    }
}
