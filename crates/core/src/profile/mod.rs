mod deprecation;
mod parent;

use crate::eapi::Eapi;
use crate::files::{
    PackageEntries, SysPackageEntries, UseEntries, entry::Precedence, pkguse::PackageUseEntries,
};
use crate::makenv::MakeEnv;
use crate::profile::deprecation::DeprecationInfo;
use crate::profile::parent::ParentEntry;
use crate::repository::RepoSet;
use crate::repository::Repository;
use crate::utils::Inherit;
use anyhow::{Context, Result, anyhow, bail};
use log::warn;
use std::fmt;
use std::path::{Path, PathBuf};

/// Identifies a profile by its canonical path and owning repository.
struct ProfileSource<'repo> {
    path: PathBuf,
    owning_repo: &'repo Repository,
}

impl<'repo> ProfileSource<'repo> {
    /// Resolves a profile path and identifies its owning repository.
    fn from_path(path: &Path, repo_set: &'repo RepoSet) -> Result<Self> {
        let path = path
            .canonicalize()
            .with_context(|| anyhow!("unable to resolve profile {}", path.display()))?;

        for repository in repo_set.values() {
            let profiles_root = repository.location.join("profiles").canonicalize()?;
            if path.starts_with(&profiles_root) {
                return Ok(Self {
                    path,
                    owning_repo: repository,
                });
            }
        }

        bail!("profile {} is not owned by any repository", path.display())
    }
}

impl fmt::Display for ProfileSource<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path.display())
    }
}

/// Represents a profile outlined in PMS section 5.
///
/// A profile shouldn't be used to check whether a package or USE flag is masked,
/// it is only temporary to resolve the final configuration.
#[derive(Default)]
pub struct Profile {
    pub location: PathBuf,
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
    /// Returns `Err` if `location` doesn't exist, the profile directory is invalid or
    /// if the profile is not valid.
    pub fn resolve(location: &Path, repo_set: &RepoSet) -> Result<Self> {
        let source = ProfileSource::from_path(location, repo_set)?;

        let mut parents = Vec::new();
        Self::build_parents(&source, repo_set, &mut parents)
            .with_context(|| anyhow!("unable to resolve parents for {source}"))?;

        let profile = Self::load(&source, Precedence::Profile(parents.len()))?;
        if parents.is_empty() {
            return Ok(profile);
        }

        let mut inherited = parents.remove(0);
        for parent in parents {
            inherited = parent.inherit(&inherited)?;
        }

        profile.inherit(&inherited)
    }

    /// Loads one profile directory without resolving its parents.
    /// The passed `order` must be the order in the inheritance chain.
    fn load(source: &ProfileSource<'_>, order: Precedence) -> Result<Self> {
        let path = &source.path;
        let eapi = Eapi::from_eapi_file(&path.join("eapi"))?;
        let supports_file_dirs = eapi.supports_profile_file_dirs()
            || source.owning_repo.layout.supports_profile_file_dirs();

        let profile = Self {
            make_defaults: MakeEnv::from_path(&path.join("make.defaults"), false, true)?,
            deprecated: DeprecationInfo::from_path(&path.join("deprecated"))?,
            packages: SysPackageEntries::from_path(&path.join("packages"), order, false)?,
            package_mask: PackageEntries::from_path(
                &path.join("package.mask"),
                order,
                supports_file_dirs,
            )?,
            package_unmask: PackageEntries::from_path(
                &path.join("package.unmask"),
                order,
                supports_file_dirs,
            )?,
            package_use: PackageUseEntries::from_path(
                &path.join("package.use"),
                order,
                supports_file_dirs,
            )?,
            use_mask: UseEntries::from_path(&path.join("use.mask"), order, supports_file_dirs)?,
            use_force: UseEntries::from_path(&path.join("use.force"), order, supports_file_dirs)?,
            use_stable_mask: UseEntries::from_path(
                &path.join("use.stable.mask"),
                order,
                supports_file_dirs,
            )?,
            use_stable_force: UseEntries::from_path(
                &path.join("use.stable.force"),
                order,
                supports_file_dirs,
            )?,
            package_use_mask: PackageUseEntries::from_path(
                &path.join("package.use.mask"),
                order,
                supports_file_dirs,
            )?,
            package_use_force: PackageUseEntries::from_path(
                &path.join("package.use.force"),
                order,
                supports_file_dirs,
            )?,
            package_use_stable_mask: PackageUseEntries::from_path(
                &path.join("package.use.stable.mask"),
                order,
                supports_file_dirs,
            )?,
            package_use_stable_force: PackageUseEntries::from_path(
                &path.join("package.use.stable.force"),
                order,
                supports_file_dirs,
            )?,
            location: path.clone(),
        };
        if let Some(deprecation) = &profile.deprecated {
            warn!(
                "This profile is deprecated. The recommended profile to upgrade to is {}\n\n{}",
                deprecation.recommended_profile, deprecation.info
            );
        }
        Ok(profile)
    }

    /// Builds all parent profiles in inheritance order.
    fn build_parents<'repo>(
        source: &ProfileSource<'repo>,
        repo_set: &'repo RepoSet,
        profiles: &mut Vec<Self>,
    ) -> Result<()> {
        for parent in ParentEntry::from_parent_file(&source.path.join("parent"))? {
            let parent_source = parent.resolve(source, repo_set).with_context(|| {
                anyhow!("invalid parent reference '{parent}' in profile {source}")
            })?;
            Self::build_parents(&parent_source, repo_set, profiles)?;

            let order = Precedence::Profile(profiles.len());
            let profile = Self::load(&parent_source, order)
                .with_context(|| anyhow!("unable to build profile from {parent_source}"))?;
            profiles.push(profile);
        }
        Ok(())
    }
}

impl Inherit for Profile {
    /// Inherits relevant configurations from the given parent profile.
    fn inherit_from(&mut self, parent: &Profile) -> anyhow::Result<()> {
        self.make_defaults.inherit_from(&parent.make_defaults)?;
        self.packages.inherit_from(&parent.packages)?;
        self.package_mask.inherit_from(&parent.package_mask)?;
        self.package_unmask.inherit_from(&parent.package_unmask)?;
        self.package_use.inherit_from(&parent.package_use)?;
        self.use_mask.inherit_from(&parent.use_mask)?;
        self.use_force.inherit_from(&parent.use_force)?;
        self.use_stable_mask.inherit_from(&parent.use_stable_mask)?;
        self.use_stable_force
            .inherit_from(&parent.use_stable_force)?;
        self.package_use_mask
            .inherit_from(&parent.package_use_mask)?;
        self.package_use_force
            .inherit_from(&parent.package_use_force)?;
        self.package_use_stable_mask
            .inherit_from(&parent.package_use_stable_mask)?;
        self.package_use_stable_force
            .inherit_from(&parent.package_use_stable_force)?;
        Ok(())
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
    use crate::files::entry::Entry;
    use crate::files::pkguse::EntryUseFlags;
    use crate::repository::test_support::{RepoBuilder, repo_set};
    use std::fs;
    use std::path::{Path, PathBuf};

    fn profile_path(repository: &Path, profile: &str) -> PathBuf {
        repository.join("profiles").join(profile)
    }

    fn assert_parent_case(format: &str, parent: &str, succeeds: bool) -> Result<()> {
        let fixture = repo_set(vec![
            RepoBuilder::new("source")
                .formats([format])
                .profile("base")
                .parents("child", [parent]),
            RepoBuilder::new("target").formats(["pms"]).profile("base"),
        ])?;

        let source_path = fixture.get("source").unwrap().location.as_path();
        let selected = profile_path(source_path, "child");

        assert_eq!(Profile::resolve(&selected, &fixture).is_ok(), succeeds);
        Ok(())
    }

    #[test]
    fn test_profile_inherit_from() -> Result<()> {
        let parent = Profile {
            make_defaults: MakeEnv::from_string("USE=\"foo\"".into())?,
            package_use_mask: PackageUseEntries::from_string(
                "sys-libs/glibc cet stack-realign".into(),
                Precedence::Profile(0),
            )?,
            use_mask: UseEntries::from_string("foo\nbar".into(), Precedence::Profile(0))?,
            ..Default::default()
        };

        let mut child = Profile {
            make_defaults: MakeEnv::from_string("USE=\"bar\"".into())?,
            use_mask: UseEntries::from_string("-bar\nbaz".into(), Precedence::Profile(1))?,
            package_use_mask: PackageUseEntries::from_string(
                "sys-libs/glibc -stack-realign\ndev-lang/rust baz".into(),
                Precedence::Profile(1),
            )?,
            ..Default::default()
        };

        child.inherit_from(&parent)?;
        assert_eq!(
            child.make_defaults.get("USE").unwrap().to_string(),
            "foo bar"
        );
        assert_eq!(
            child.use_mask.into_iter().collect::<Vec<_>>(),
            vec![
                Entry::from_str("foo", Precedence::Profile(0))?,
                Entry::from_str("-bar", Precedence::Profile(1))?,
                Entry::from_str("baz", Precedence::Profile(1))?,
            ]
        );
        assert_eq!(
            child.package_use_mask.into_inner(),
            [
                (
                    "sys-libs/glibc".parse()?,
                    EntryUseFlags::from_raw(vec![
                        Entry::from_str("cet", Precedence::Profile(0))?,
                        Entry::from_str("-stack-realign", Precedence::Profile(1))?,
                    ])
                ),
                (
                    "dev-lang/rust".parse()?,
                    EntryUseFlags::from_raw(vec![Entry::from_str("baz", Precedence::Profile(1))?])
                )
            ]
            .into_iter()
            .collect()
        );
        Ok(())
    }

    #[test]
    fn test_profile_directories() -> Result<()> {
        let cases = [
            ("pms-eapi-0", "pms", "0", false),
            ("portage-1-eapi-0", "portage-1", "0", true),
            ("pms-eapi-7", "pms", "7", true),
        ];

        for (name, format, eapi, succeeds) in cases {
            let fixture = repo_set(vec![
                RepoBuilder::new(name)
                    .formats([format])
                    .profile_eapi("selected", eapi)
                    .profile_entries_dir("selected/use.mask", "test\n"),
            ])?;

            let repo_path = fixture.get(name).unwrap().location.as_path();
            let selected = profile_path(repo_path, "selected");

            assert_eq!(Profile::resolve(&selected, &fixture).is_ok(), succeeds);
        }
        Ok(())
    }

    #[test]
    fn test_packages_are_file_only() -> Result<()> {
        let fixture = repo_set(vec![
            RepoBuilder::new("repo")
                .formats(["portage-2"])
                .profile_eapi("selected", "8")
                .profile_entries_dir("selected/packages", "sys-apps/coreutils\n"),
        ])?;

        let repo_path = fixture.get("repo").unwrap().location.as_path();
        let selected = profile_path(repo_path, "selected");

        assert!(Profile::resolve(&selected, &fixture).is_err());
        Ok(())
    }

    #[test]
    fn test_parent_formats() -> Result<()> {
        for format in ["pms", "portage-1", "portage-2"] {
            assert_parent_case(format, "../base", true)?;
            assert_parent_case(format, "target:base", format == "portage-2")?;
            assert_parent_case(format, ":base", format == "portage-2")?;
        }
        Ok(())
    }

    #[test]
    fn test_root_parent_escape() -> Result<()> {
        let fixture = repo_set(vec![
            RepoBuilder::new("source")
                .formats(["portage-2"])
                .parents("root-relative", [":../outside"])
                .parents("ordinary-relative", ["../../outside"]),
        ])?;

        let source_path = fixture.get("source").unwrap().location.as_path();
        fs::create_dir(source_path.join("outside"))?;

        assert!(Profile::resolve(&profile_path(source_path, "root-relative"), &fixture).is_err());
        assert!(
            Profile::resolve(&profile_path(source_path, "ordinary-relative"), &fixture).is_err()
        );
        Ok(())
    }
}
