mod deprecation;

use crate::conf::repos::ReposConf;
use crate::eapi::Eapi;
use crate::linefile::LineBasedFile;
use crate::makenv::MakeEnv;
use crate::profile::deprecation::DeprecationInfo;
use crate::utils::{FileFromPath, Inherit};
use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::Path;

/// Represents a profile outlined in PMS section 5.
#[derive(Default, Debug)]
pub struct Profile {
    eapi: Eapi,
    deprecated: Option<DeprecationInfo>,

    pub make_defaults: MakeEnv,

    packages: LineBasedFile,
    // Prevents packages from being installed in this profile
    pub package_mask: LineBasedFile,
    // Allows packages to be installed that would otherwise be masked
    pub package_unmask: LineBasedFile,
    // Override the default USE flags specified by make.defaults on a per-package basis
    package_use: LineBasedFile,

    // USE flags that must never be enabled in this profile
    use_mask: LineBasedFile,
    // USE flags that must always be enabled in this profile
    use_force: LineBasedFile,
    // Same as above but for merged packages due to a stable keyword
    use_stable_mask: LineBasedFile,
    use_stable_force: LineBasedFile,

    // USE flags that must never be enabled on a per-package or per-version basis
    package_use_mask: LineBasedFile,
    // USE flags that must always be enabled on a per-package or per-version basis
    package_use_force: LineBasedFile,
    // Same as above but for merged packages due to a stable keyword
    package_use_stable_mask: LineBasedFile,
    package_use_stable_force: LineBasedFile,
}

impl Profile {
    /// Builds a profile from the given `path` and all available repositories from `repos`.
    /// An error is returned if the `path` doesn't exist or the profile directory is invalid.
    pub fn new(path: &Path, repos: &ReposConf) -> Result<Self> {
        let eapi = Self::read_eapi(&path.join("eapi"))?;

        // TODO: remove debug print
        println!(
            "Loading profile from {} (EAPI: {})",
            path.canonicalize()?.display(),
            eapi
        );

        let mut profile = Self {
            make_defaults: MakeEnv::from_path(&path.join("make.defaults"), false, true)?,
            deprecated: DeprecationInfo::from_path(&path.join("deprecated"))?,
            packages: LineBasedFile::from_path(
                &path.join("packages"),
                eapi.profile_file_dirs,
                true,
            )?,
            package_mask: LineBasedFile::from_path(
                &path.join("package.mask"),
                eapi.profile_file_dirs,
                true,
            )?,
            package_unmask: LineBasedFile::from_path(
                &path.join("package.unmask"),
                eapi.profile_file_dirs,
                true,
            )?,
            package_use: LineBasedFile::from_path(
                &path.join("package.use"),
                eapi.profile_file_dirs,
                true,
            )?,
            use_mask: LineBasedFile::from_path(
                &path.join("use.mask"),
                eapi.profile_file_dirs,
                true,
            )?,
            use_force: LineBasedFile::from_path(
                &path.join("use.force"),
                eapi.profile_file_dirs,
                true,
            )?,
            use_stable_mask: LineBasedFile::from_path(
                &path.join("use.stable.mask"),
                eapi.profile_file_dirs,
                true,
            )?,
            use_stable_force: LineBasedFile::from_path(
                &path.join("use.stable.force"),
                eapi.profile_file_dirs,
                true,
            )?,
            package_use_mask: LineBasedFile::from_path(
                &path.join("package.use.mask"),
                eapi.profile_file_dirs,
                true,
            )?,
            package_use_force: LineBasedFile::from_path(
                &path.join("package.use.force"),
                eapi.profile_file_dirs,
                true,
            )?,
            package_use_stable_mask: LineBasedFile::from_path(
                &path.join("package.use.stable.mask"),
                eapi.profile_file_dirs,
                true,
            )?,
            package_use_stable_force: LineBasedFile::from_path(
                &path.join("package.use.stable.force"),
                eapi.profile_file_dirs,
                true,
            )?,
            eapi,
        };
        for parent in Self::resolve_parents(path, repos)? {
            profile.inherit_from(&parent);
        }

        if let Some(deprecation) = &profile.deprecated {
            eprintln!(
                "This profile is deprecated. The recommended profile to upgrade to is {}\n\n{}",
                deprecation.recommended_profile, deprecation.info
            );
        }
        Ok(profile)
    }

    /// Takes a `path` to a profile directory and resolves all profiles listed in the parent file.
    /// Parents are returned in the order they are listed or an empty vec if there are none.
    fn resolve_parents(path: &Path, repos: &ReposConf) -> Result<Vec<Self>> {
        let parent = path.join("parent");
        if !parent.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(parent).with_context(|| "Unable to read parent file")?;
        let parent_profiles = content
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect::<Vec<_>>();

        let mut profiles = Vec::new();
        for profile in parent_profiles {
            // If a profiles references a specific repository, resolve it first.
            // This is not specified in PMS but is implemented in portage
            // see https://bugs.gentoo.org/515666
            let path = match profile.split_once(':') {
                Some((repo_name, profile_path)) => {
                    let repo = repos.get(repo_name).ok_or_else(|| {
                        anyhow!("Repository '{repo_name}' not found for profile '{profile}'")
                    })?;
                    repo.location.join("profiles").join(profile_path)
                }
                None => path.join(profile),
            };
            profiles.push(Profile::new(&path, repos)?);
        }
        Ok(profiles)
    }

    /// Reads the EAPI version from the given file `path`.
    fn read_eapi(path: &Path) -> Result<Eapi> {
        if !path.exists() {
            return Ok(Eapi::default());
        }
        Eapi::new(
            fs::read_to_string(path)
                .with_context(|| "Unable to read eapi file")?
                .lines()
                .next()
                .ok_or_else(|| anyhow!("Empty eapi file"))?,
        )
    }
}

impl Inherit for Profile {
    /// Inherits relevant configurations from the given parent profile.
    fn inherit_from(&mut self, parent: &Profile) {
        self.make_defaults.inherit_from(&parent.make_defaults);
        self.packages.inherit_from(&parent.packages);
        self.package_mask.inherit_from(&parent.package_mask);
        self.package_unmask.inherit_from(&parent.package_unmask);
        self.package_use.inherit_from(&parent.package_use);
        self.use_mask.inherit_from(&parent.use_mask);
        self.use_force.inherit_from(&parent.use_force);
        self.use_stable_mask.inherit_from(&parent.use_stable_mask);
        self.use_stable_force.inherit_from(&parent.use_stable_force);
        self.package_use_mask.inherit_from(&parent.package_use_mask);
        self.package_use_force
            .inherit_from(&parent.package_use_force);
        self.package_use_stable_mask
            .inherit_from(&parent.package_use_stable_mask);
        self.package_use_stable_force
            .inherit_from(&parent.package_use_stable_force);
    }
}
