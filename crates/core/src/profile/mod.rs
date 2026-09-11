mod deprecation;
mod parent;

use crate::eapi::Eapi;
use crate::files::pkgfile::{PackageAcceptKeywords, PackageUseRecords};
use crate::files::{PackageEntries, SysPackageEntries, UseEntries, entry::Precedence};
use crate::makenv::MakeEnv;
use crate::profile::deprecation::DeprecationInfo;
use crate::profile::parent::ParentEntry;
use crate::repository::{RepoSet, Repository};
use crate::useflag::UseExpandConfig;
use crate::utils::{DfsState, Inherit};
use anyhow::{Context, bail};
use log::warn;
use std::path::{Path, PathBuf};
use std::{fmt, iter};

/// Identifies a profile by its canonical path and owning repository.
struct ProfileSource<'repo> {
    path: PathBuf,
    owning_repo: &'repo Repository,
}

impl<'repo> ProfileSource<'repo> {
    /// Resolves a profile path and identifies its owning repository.
    fn from_path(path: &Path, repo_set: &'repo RepoSet) -> anyhow::Result<Self> {
        let path = path
            .canonicalize()
            .with_context(|| format!("unable to resolve profile {}", path.display()))?;

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

    /// Defines a system set for this profile
    packages: SysPackageEntries,

    /// Defines keywords that are accepted for packages in this profile
    pub package_accept_keywords: PackageAcceptKeywords,

    /// Masks packages from being visible
    pub package_mask: PackageEntries,
    /// Changes visibility of packages that would otherwise be masked
    pub package_unmask: PackageEntries,

    /// Parsed and inherited profile USE records for the resolved profile.
    pub use_records: ProfileUseRecords,
}

impl Profile {
    /// Resolves a profile from the given `location` and takes care of inheriting all parents.
    /// `repo_set` is used to resolve profiles in different repositories.
    ///
    /// Returns `Err` if `location` doesn't exist, the profile directory is invalid or
    /// if the profile is not valid.
    pub fn resolve(location: &Path, repo_set: &RepoSet) -> anyhow::Result<Self> {
        let source = ProfileSource::from_path(location, repo_set)?;

        let mut parents = Vec::new();
        let mut dfs = DfsState::default();
        Self::build_parents(&source, repo_set, &mut parents, &mut dfs)
            .with_context(|| format!("unable to resolve parents for {source}"))?;

        // Fold make.defaults layers before inheriting other profile files so active
        // USE_EXPAND member variables use incremental semantics.
        let mut profile = Self::load(&source, Precedence::Profile(parents.len()))?;
        let layers = parents
            .iter()
            .map(|p| &p.make_defaults)
            .chain(iter::once(&profile.make_defaults))
            .collect::<Vec<_>>();
        profile.make_defaults = MakeEnv::fold_with_use_expand(&layers)?;
        profile
            .use_records
            .set_expand_config(&profile.make_defaults)?;

        let parents = Self::resolve_parents(parents)?;
        profile.inherit(&parents)
    }

    /// Loads one profile directory without resolving its parents.
    /// The passed `order` must be the order in the inheritance chain.
    fn load(source: &ProfileSource<'_>, order: Precedence) -> anyhow::Result<Self> {
        let path = &source.path;
        let eapi = Eapi::from_eapi_file(&path.join("eapi"))?;
        let recursive = eapi.supports_profile_file_dirs()
            || source.owning_repo.layout.supports_profile_file_dirs();

        let profile = Self {
            make_defaults: MakeEnv::from_path(&path.join("make.defaults"), false, true)?,
            deprecated: DeprecationInfo::from_path(&path.join("deprecated"))?,
            packages: SysPackageEntries::from_path(&path.join("packages"), order, false)?,
            package_accept_keywords: PackageAcceptKeywords::from_path(
                &path.join("package.accept_keywords"),
                order,
                recursive,
            )?,
            package_mask: PackageEntries::from_path(&path.join("package.mask"), order, recursive)?,
            package_unmask: PackageEntries::from_path(
                &path.join("package.unmask"),
                order,
                recursive,
            )?,
            use_records: ProfileUseRecords::load(path, order, recursive)?,
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

    /// Inherits the given `parent` profile, except `make_defaults`
    /// which needs to be handled beforehand to distinguish between
    /// incremental and literal variables.
    fn inherit(mut self, parent: &Profile) -> anyhow::Result<Self> {
        self.packages.inherit_from(&parent.packages)?;
        self.package_accept_keywords
            .inherit_from(&parent.package_accept_keywords)?;
        self.package_mask.inherit_from(&parent.package_mask)?;
        self.package_unmask.inherit_from(&parent.package_unmask)?;
        self.use_records.inherit_from(&parent.use_records)?;
        Ok(self)
    }

    /// Builds all parent profiles in inheritance order (depth first, left to right)
    /// and stores them in `profiles`.
    fn build_parents<'repo>(
        source: &ProfileSource<'repo>,
        repo_set: &'repo RepoSet,
        profiles: &mut Vec<Self>,
        dfs: &mut DfsState<PathBuf>,
    ) -> anyhow::Result<()> {
        if let Err(cycle) = dfs.enter(&source.path) {
            let path = cycle
                .path
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(" -> ");
            bail!("profile parent cycle detected involving {path}");
        }

        for parent in ParentEntry::from_parent_file(&source.path.join("parent"))? {
            let source = parent.resolve(source, repo_set).with_context(|| {
                format!("invalid parent reference '{parent}' in profile {source}")
            })?;
            Self::build_parents(&source, repo_set, profiles, dfs)?;

            let order = Precedence::Profile(profiles.len());
            let profile = Self::load(&source, order)
                .with_context(|| format!("unable to build profile from {source}"))?;
            profiles.push(profile);
        }
        dfs.leave(&source.path);
        Ok(())
    }

    /// Inherits all given `parents` and returns a resolved parent [`Profile`].
    ///
    /// NOTE: The caller must make sure the parents are in the correct order:
    /// depth first, left to right.
    fn resolve_parents(parents: Vec<Self>) -> anyhow::Result<Self> {
        parents
            .into_iter()
            .try_fold(Self::default(), |resolved_parent, parent| {
                parent.inherit(&resolved_parent)
            })
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.location.display())
    }
}

/// Parsed and inherited profile USE records for the resolved profile.
#[derive(Clone, Default)]
pub struct ProfileUseRecords {
    pub package_use: PackageUseRecords,
    pub package_use_mask: PackageUseRecords,
    pub package_use_force: PackageUseRecords,
    pub package_use_stable_mask: PackageUseRecords,
    pub package_use_stable_force: PackageUseRecords,
    pub use_mask: UseEntries,
    pub use_force: UseEntries,
    pub use_stable_mask: UseEntries,
    pub use_stable_force: UseEntries,
    pub expand_config: UseExpandConfig,
}

impl ProfileUseRecords {
    fn load(path: &Path, order: Precedence, recursive: bool) -> anyhow::Result<Self> {
        Ok(Self {
            package_use: PackageUseRecords::from_path(&path.join("package.use"), order, recursive)?,
            use_mask: UseEntries::from_path(&path.join("use.mask"), order, recursive)?,
            use_force: UseEntries::from_path(&path.join("use.force"), order, recursive)?,
            use_stable_mask: UseEntries::from_path(
                &path.join("use.stable.mask"),
                order,
                recursive,
            )?,
            use_stable_force: UseEntries::from_path(
                &path.join("use.stable.force"),
                order,
                recursive,
            )?,
            package_use_mask: PackageUseRecords::from_path(
                &path.join("package.use.mask"),
                order,
                recursive,
            )?,
            package_use_force: PackageUseRecords::from_path(
                &path.join("package.use.force"),
                order,
                recursive,
            )?,
            package_use_stable_mask: PackageUseRecords::from_path(
                &path.join("package.use.stable.mask"),
                order,
                recursive,
            )?,
            package_use_stable_force: PackageUseRecords::from_path(
                &path.join("package.use.stable.force"),
                order,
                recursive,
            )?,
            expand_config: UseExpandConfig::default(),
        })
    }

    fn inherit_from(&mut self, parent: &Self) -> anyhow::Result<()> {
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

    fn set_expand_config(&mut self, make_defaults: &MakeEnv) -> anyhow::Result<()> {
        self.expand_config = UseExpandConfig::from_makenv(make_defaults)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::entry::Entry;
    use crate::files::pkgfile::KeywordRule;

    use crate::repository::test_support::{RepoBuilder, repo_set};
    use std::fs;

    fn profile_path(repository: &Path, profile: &str) -> PathBuf {
        repository.join("profiles").join(profile)
    }

    fn assert_parent_case(format: &str, parent: &str, succeeds: bool) -> anyhow::Result<()> {
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

    fn assert_package_accept_keywords(profile: &Profile) -> anyhow::Result<()> {
        let rust_atom = "dev-lang/rust".parse()?;
        let rust_keywords = profile
            .package_accept_keywords
            .clone()
            .into_rules()
            .filter_map(|(atom, rule)| (atom == rust_atom).then_some(rule))
            .collect::<Vec<_>>();
        assert_eq!(
            rust_keywords,
            [
                KeywordRule::Selector(Entry::from_str("amd64", Precedence::Profile(0))?),
                KeywordRule::Selector(Entry::from_str("~amd64", Precedence::Profile(1))?),
                KeywordRule::Selector(Entry::from_str("**", Precedence::Profile(2))?),
            ]
        );

        let vim_atom = "app-editors/vim".parse()?;
        let vim_keywords = profile
            .package_accept_keywords
            .clone()
            .into_rules()
            .filter_map(|(atom, rule)| (atom == vim_atom).then_some(rule))
            .collect::<Vec<_>>();
        assert_eq!(
            vim_keywords,
            [
                KeywordRule::Reset(Precedence::Profile(1)),
                KeywordRule::Selector(Entry::from_str("~amd64", Precedence::Profile(1))?),
            ]
        );
        Ok(())
    }

    #[test]
    fn test_profile_resolve() -> anyhow::Result<()> {
        let fixture = repo_set(vec![
            RepoBuilder::new("repo")
                .formats(["pms"])
                .profile("base")
                .profile("parent")
                .profile("selected")
                .parents("parent", ["../base"])
                .parents("selected", ["../parent"])
                .profile_file(
                    "base/make.defaults",
                    "USE_EXPAND=\"CAMERAS LLVM_TARGETS\"\n\
                    CAMERAS=\"canon ptp2\"\n",
                )
                .profile_file("parent/make.defaults", "CAMERAS=\"-canon nikon\"\n")
                .profile_file("base/use.mask", "foo\nbar\n")
                .profile_file("parent/use.mask", "-bar\nbaz\n")
                .profile_file("selected/use.mask", "-bar\nqux\n")
                .profile_file(
                    "base/package.accept_keywords",
                    "dev-lang/rust amd64\n\
                    app-editors/vim amd64\n",
                )
                .profile_file(
                    "parent/package.accept_keywords",
                    "dev-lang/rust ~amd64\n\
                    app-editors/vim -* ~amd64\n",
                )
                .profile_file("selected/package.accept_keywords", "dev-lang/rust **\n")
                .profile_file(
                    "base/package.use.mask",
                    "sys-libs/glibc cet stack-realign\n\
                     dev-lang/rust LLVM_TARGETS: X86\n",
                )
                .profile_file(
                    "parent/package.use.mask",
                    "sys-libs/glibc -stack-realign\n\
                     dev-lang/rust LLVM_TARGETS: -* AMDGPU\n",
                )
                .profile_file("selected/package.use.mask", "dev-lang/rust baz\n"),
        ])?;

        let repository = fixture.get("repo").unwrap();
        let selected = profile_path(&repository.location, "selected");
        let profile = Profile::resolve(&selected, &fixture)?;

        assert_eq!(
            profile.make_defaults.get("CAMERAS").unwrap().to_string(),
            "ptp2 nikon"
        );
        assert_package_accept_keywords(&profile)?;

        assert_eq!(
            profile.use_records.use_mask.into_iter().collect::<Vec<_>>(),
            vec![
                Entry::from_str("foo", Precedence::Profile(0))?,
                Entry::from_str("baz", Precedence::Profile(1))?,
                Entry::from_str("-bar", Precedence::Profile(2))?,
                Entry::from_str("qux", Precedence::Profile(2))?,
            ]
        );
        let package_use_mask = profile
            .use_records
            .package_use_mask
            .expand(&profile.use_records.expand_config)?;
        let glibc_atom = "sys-libs/glibc".parse()?;
        let glibc = package_use_mask
            .iter()
            .find_map(|(atom, flags)| (atom == &glibc_atom).then_some(flags))
            .unwrap();
        assert_eq!(
            glibc.get(&"cet".parse()?),
            Some(&Entry::from_str("cet", Precedence::Profile(0))?)
        );
        assert_eq!(
            glibc.get(&"stack-realign".parse()?),
            Some(&Entry::from_str("-stack-realign", Precedence::Profile(1))?)
        );

        let rust_atom = "dev-lang/rust".parse()?;
        let rust = package_use_mask
            .iter()
            .find_map(|(atom, flags)| (atom == &rust_atom).then_some(flags))
            .unwrap();
        assert_eq!(
            rust.get(&"llvm_targets_AMDGPU".parse()?),
            Some(&Entry::from_str(
                "llvm_targets_AMDGPU",
                Precedence::Profile(1)
            )?)
        );
        assert_eq!(rust.get(&"llvm_targets_X86".parse()?), None);
        assert_eq!(
            rust.get(&"baz".parse()?),
            Some(&Entry::from_str("baz", Precedence::Profile(2))?)
        );
        Ok(())
    }

    #[test]
    fn test_profile_directories() -> anyhow::Result<()> {
        let cases = [
            ("pms-eapi", "pms", "0", false),
            ("portage-one-eapi", "portage-1", "0", true),
            ("pms-eapi-seven", "pms", "7", true),
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
    fn test_packages_are_file_only() -> anyhow::Result<()> {
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
    fn test_parent_formats() -> anyhow::Result<()> {
        for format in ["pms", "portage-1", "portage-2"] {
            assert_parent_case(format, "../base", true)?;
            assert_parent_case(format, "target:base", format == "portage-2")?;
            assert_parent_case(format, ":base", format == "portage-2")?;
        }
        Ok(())
    }

    #[test]
    fn test_root_parent_escape() -> anyhow::Result<()> {
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

    #[test]
    fn test_parent_direct_cycle() {
        let fixture = repo_set(vec![
            RepoBuilder::new("repo")
                .formats(["pms"])
                .profile("selected")
                .parents("selected", ["../selected"]),
        ])
        .unwrap();
        let repo_path = fixture.get("repo").unwrap().location.as_path();

        assert!(Profile::resolve(&profile_path(repo_path, "selected"), &fixture).is_err());
    }

    #[test]
    fn test_parent_indirect_cycle() {
        let fixture = repo_set(vec![
            RepoBuilder::new("repo")
                .formats(["pms"])
                .profile("first")
                .profile("second")
                .profile("third")
                .parents("first", ["../second"])
                .parents("second", ["../third"])
                .parents("third", ["../first"]),
        ])
        .unwrap();
        let repo_path = fixture.get("repo").unwrap().location.as_path();

        assert!(Profile::resolve(&profile_path(repo_path, "first"), &fixture).is_err());
    }
}
