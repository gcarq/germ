mod deprecation;

use crate::eapi::Eapi;
use crate::files::pkguse::PackageUseEntries;
use crate::files::{FileFromPath, PackageEntries, SysPackageEntries, UseEntries};
use crate::makenv::MakeEnv;
use crate::profile::deprecation::DeprecationInfo;
use crate::repository::set::RepoSet;
use crate::utils::Inherit;
use anyhow::{Context, Result, anyhow};
use log::warn;
use std::path::{Path, PathBuf};
use std::{fmt, fs};

/// Represents a profile outlined in PMS section 5.
///
/// A profile shouldn't be used to check whether a package or USE flag is masked,
/// it is only temporary to resolve the final configuration.
#[derive(Default)]
pub struct Profile {
    pub location: PathBuf,
    eapi: Eapi,
    deprecated: Option<DeprecationInfo>,

    pub make_defaults: MakeEnv,

    // Defines a system set for this profile
    packages: SysPackageEntries,
    // Prevents packages from being installed in this profile
    pub package_mask: PackageEntries,
    // Allows packages to be installed that would otherwise be masked
    pub package_unmask: PackageEntries,

    // Override the default USE flags specified by make.defaults on a per-package basis
    pub package_use: PackageUseEntries,
    // USE flags that must never be enabled on a per-package or per-version basis
    pub package_use_mask: PackageUseEntries,
    // USE flags that must always be enabled on a per-package or per-version basis
    pub package_use_force: PackageUseEntries,
    // Same as above but for merged packages due to a stable keyword
    pub package_use_stable_mask: PackageUseEntries,
    pub package_use_stable_force: PackageUseEntries,

    // USE flags that must never be enabled in this profile
    pub use_mask: UseEntries,
    // USE flags that must always be enabled in this profile
    pub use_force: UseEntries,
    // Same as above but for merged packages due to a stable keyword
    pub use_stable_mask: UseEntries,
    pub use_stable_force: UseEntries,
}

impl Profile {
    /// Resolves a profile from the given `location` and takes care of inheriting all parents.
    /// `repo_set` is used to resolve profiles in different repositories.
    ///
    /// Returns `Err` if `location` doesn't exist or the profile directory is invalid.
    pub fn resolve(location: &Path, repo_set: &RepoSet) -> Result<Self> {
        let mut profile = Self::new(location)?;

        let parents = Self::resolve_parents(location, repo_set)
            .with_context(|| anyhow!("unable to resolve parents for {}", location.display()))?;

        if let Some((base, parents)) = parents.split_first() {
            let mut prev = Self::new(base)
                .with_context(|| anyhow!("unable to build profile from {}", base.display()))?;
            for next in parents {
                let next = Self::new(next)
                    .with_context(|| anyhow!("unable to build profile from {}", next.display()))?;
                prev = next.inherit(&prev);
            }
            profile.inherit_from(&prev);
        }
        Ok(profile)
    }

    /// Builds a profile from the given `location`.
    ///
    /// Returns `Err` if `location` doesn't exist or the profile directory is invalid.
    fn new(location: &Path) -> Result<Self> {
        let eapi = Self::read_eapi(&location.join("eapi"))?;
        let dir_support = eapi.supports_profile_file_dirs();
        let profile = Self {
            make_defaults: MakeEnv::from_path(&location.join("make.defaults"), false, true)?,
            deprecated: DeprecationInfo::from_path(&location.join("deprecated"))?,
            packages: SysPackageEntries::from_path(&location.join("packages"), dir_support, true)?,
            package_mask: PackageEntries::from_path(
                &location.join("package.mask"),
                dir_support,
                true,
            )?,
            package_unmask: PackageEntries::from_path(
                &location.join("package.unmask"),
                dir_support,
                true,
            )?,
            package_use: PackageUseEntries::from_path(
                &location.join("package.use"),
                dir_support,
                true,
            )?,
            use_mask: UseEntries::from_path(&location.join("use.mask"), dir_support, true)?,
            use_force: UseEntries::from_path(&location.join("use.force"), dir_support, true)?,
            use_stable_mask: UseEntries::from_path(
                &location.join("use.stable.mask"),
                dir_support,
                true,
            )?,
            use_stable_force: UseEntries::from_path(
                &location.join("use.stable.force"),
                dir_support,
                true,
            )?,
            package_use_mask: PackageUseEntries::from_path(
                &location.join("package.use.mask"),
                dir_support,
                true,
            )?,
            package_use_force: PackageUseEntries::from_path(
                &location.join("package.use.force"),
                dir_support,
                true,
            )?,
            package_use_stable_mask: PackageUseEntries::from_path(
                &location.join("package.use.stable.mask"),
                dir_support,
                true,
            )?,
            package_use_stable_force: PackageUseEntries::from_path(
                &location.join("package.use.stable.force"),
                dir_support,
                true,
            )?,
            eapi,
            location: location.canonicalize()?,
        };
        if let Some(deprecation) = &profile.deprecated {
            warn!(
                "This profile is deprecated. The recommended profile to upgrade to is {}\n\n{}",
                deprecation.recommended_profile, deprecation.info
            );
        }
        Ok(profile)
    }

    /// Takes a `path` to a profile directory and resolves all profile paths it inherits from.
    /// `repo_set` is used to resolve profiles in different repositories.
    fn resolve_parents(path: &Path, repo_set: &RepoSet) -> Result<Vec<PathBuf>> {
        let parent = path.join("parent");
        if !parent.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(parent).with_context(|| "unable to read parent file")?;
        let parent_profiles = content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'));

        let mut paths = Vec::new();
        for profile in parent_profiles {
            // If a profiles references a specific repository, resolve it first.
            // This is not specified in PMS but is implemented in portage
            // see https://bugs.gentoo.org/515666
            // TODO: this behavior is controlled via profile-formats in <repo>/metadata/layout.conf
            //  see https://wiki.gentoo.org/wiki/Portage/Profiles/Custom_profiles
            let path = match profile.split_once(':') {
                Some((repo_name, profile_path)) => {
                    let repo = repo_set.get(repo_name).ok_or_else(|| {
                        anyhow!("Repository '{repo_name}' not found for profile '{profile}'")
                    })?;
                    repo.location.join("profiles").join(profile_path)
                }
                None => path.join(profile),
            };
            paths.extend(Self::resolve_parents(&path, repo_set)?);
            paths.push(path.canonicalize()?);
        }
        Ok(paths)
    }

    /// Reads the EAPI version from the given file `path`.
    fn read_eapi(path: &Path) -> Result<Eapi> {
        if !path.exists() {
            return Ok(Eapi::default());
        }
        fs::read_to_string(path)
            .with_context(|| "unable to read eapi file")?
            .lines()
            .next()
            .ok_or_else(|| anyhow!("empty eapi file"))?
            .parse()
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

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.location.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_inherit_from() -> Result<()> {
        let parent = Profile {
            make_defaults: MakeEnv::from_string("USE=\"foo\"".into())?,
            package_use_mask: PackageUseEntries::from_string(
                "sys-libs/glibc cet stack-realign".into(),
            )?,
            use_mask: UseEntries::from_string("bar".into())?,
            ..Default::default()
        };

        let mut child = Profile {
            make_defaults: MakeEnv::from_string("USE=\"bar\"".into())?,
            use_mask: UseEntries::from_string("-bar baz".into())?,
            package_use_mask: PackageUseEntries::from_string(
                "sys-libs/glibc -stack-realign\ndev-lang/rust baz".into(),
            )?,
            ..Default::default()
        };

        child.inherit_from(&parent);
        assert_eq!(
            child.make_defaults.get("USE").unwrap().to_string(),
            "foo bar"
        );
        assert_eq!(child.use_mask.finalize(), vec!["bar".parse()?]);
        assert_eq!(
            child.package_use_mask.finalize(),
            [
                (
                    "sys-libs/glibc".parse()?,
                    vec!["cet".parse()?].into_iter().collect()
                ),
                (
                    "dev-lang/rust".parse()?,
                    vec!["baz".parse()?].into_iter().collect()
                )
            ]
            .into_iter()
            .collect()
        );
        Ok(())
    }
}
