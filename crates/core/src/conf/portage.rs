use std::path::PathBuf;
use std::str::FromStr;

use crate::SysConf;
use crate::files::entry::Precedence;
use crate::files::pkgfile::{PackageAcceptKeywords, PackageUseRecords};
use crate::files::{PackageEntries, UseEntries};
use crate::makenv::MakeEnv;
use crate::policy::keyword::EffectiveKeywords;
use crate::policy::pkgmask::PortageSource as PackageMaskSource;
use crate::policy::useflag::{
    LocalRecords as UseLocalRecords, ProfileRecords as UseProfileRecords, UsePolicy,
};
use crate::profile::Profile;
use crate::repository::RepoSet;
use crate::types::FxHashSet;
use crate::useflag::UseFlag;
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

        let makenv = Self::init_make_env(&profile, sysconf)?;
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
    fn init_make_env(profile: &Profile, sysconf: &SysConf) -> anyhow::Result<MakeEnv> {
        let globals_path = sysconf.default_portage_conf().join("make.globals");
        let make_globals = MakeEnv::from_path(&globals_path, true, false)
            .context("unable to process make.globals")?;
        let make_conf = MakeEnv::from_path(&sysconf.portage_conf().join("make.conf"), true, false)
            .context("unable to process make.conf")?;

        MakeEnv::default()
            .inherit(&make_globals)?
            .inherit(&profile.make_defaults)?
            .inherit(&make_conf)
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
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::*;

    use crate::repository::test_support::RepoBuilder;

    fn fixture() -> anyhow::Result<(tempfile::TempDir, SysConf, RepoSet)> {
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
        symlink(
            repository.join("profiles/default/linux"),
            portage.join("make.profile"),
        )?;
        let globals = sysconf.default_portage_conf().join("make.globals");
        fs::create_dir_all(globals.parent().expect("make.globals parent"))?;
        fs::write(globals, "ARCH=amd64\n")?;

        let repo_set = RepoSet::new(sysconf.clone().into())?;
        Ok((root, sysconf, repo_set))
    }

    #[test]
    fn test_portage_conf_make_env() -> anyhow::Result<()> {
        let (_root, sysconf, repo_set) = fixture()?;
        let conf = PortageConf::new(&repo_set, &sysconf)?;

        assert_eq!(
            conf.makenv().get("ARCH").map(ToString::to_string),
            Some("amd64".into())
        );
        Ok(())
    }

    #[test]
    fn test_portage_conf_use_policy() -> anyhow::Result<()> {
        let (_root, sysconf, repo_set) = fixture()?;
        fs::write(
            sysconf.portage_conf().join("make.conf"),
            "IUSE_IMPLICIT=local_flag\n",
        )?;
        let conf = PortageConf::new(&repo_set, &sysconf)?;

        assert_eq!(
            conf.iuse_implicit()?,
            FxHashSet::from_iter([UseFlag::new("profile_flag")?])
        );
        Ok(())
    }

    #[test]
    fn test_portage_conf_skips_keyword_policy() -> anyhow::Result<()> {
        let (root, sysconf, repo_set) = fixture()?;
        fs::write(
            sysconf.portage_conf().join("package.accept_keywords"),
            "invalid atom",
        )?;
        let conf = PortageConf::new(&repo_set, &sysconf)?;

        assert!(conf.use_policy().is_ok());
        assert!(conf.effective_keywords().is_err());
        drop(root);
        Ok(())
    }
}
