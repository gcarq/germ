mod deprecation;
mod parent;
mod system;
#[cfg(test)]
pub(crate) mod test_support;

use std::path::{Path, PathBuf};
use std::{fmt, iter};

use anyhow::{Context, bail};
use itertools::Itertools;
use log::warn;
pub use system::SystemPackages;

use crate::eapi::read_eapi;
use crate::files::entry::Precedence;
use crate::files::pkgfile::{PackageAcceptKeywords, PackageUseRecords};
use crate::files::{AtomEntries, UseEntries};
use crate::makenv::MakeEnv;
use crate::profile::deprecation::DeprecationInfo;
use crate::profile::parent::ParentEntry;
use crate::repository::{RepoSet, Repository};
use crate::useflag::UseExpandConfig;
use crate::utils::{DfsState, Inherit};

/// Identifies a profile by its canonical path and owning repository.
struct ProfileSource<'repo> {
    path: PathBuf,
    owning_repo: &'repo Repository,
}

impl<'repo> ProfileSource<'repo> {
    /// Resolves a profile path and identifies its owning repository.
    fn from_path(path: &Path, reposet: &'repo RepoSet) -> anyhow::Result<Self> {
        let path = path
            .canonicalize()
            .with_context(|| format!("unable to resolve profile {}", path.display()))?;

        for repo in reposet.iter() {
            let profiles_root = repo.location().join("profiles").canonicalize()?;
            if path.starts_with(&profiles_root) {
                return Ok(Self {
                    path,
                    owning_repo: repo,
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

    /// The raw `@system` entries for this profile
    pub system_set: SystemPackages,

    /// Defines keywords that are accepted for packages in this profile
    pub package_accept_keywords: PackageAcceptKeywords,

    /// Masks packages from being visible
    pub package_mask: AtomEntries,
    /// Changes visibility of packages that would otherwise be masked
    pub package_unmask: AtomEntries,

    /// Parsed and inherited profile USE records for the resolved profile.
    pub use_records: ProfileUseRecords,
}

impl Profile {
    /// Resolves a profile from the given `location` and takes care of inheriting all parents.
    /// `reposet` is used to resolve profiles in different repositories.
    ///
    /// Returns `Err` if `location` doesn't exist, the profile directory is invalid or
    /// if the profile is not valid.
    pub fn resolve(location: &Path, reposet: &RepoSet) -> anyhow::Result<Self> {
        let source = ProfileSource::from_path(location, reposet)?;

        let mut parents = Vec::new();
        let mut dfs = DfsState::default();
        Self::build_parents(&source, reposet, &mut parents, &mut dfs)
            .with_context(|| format!("unable to resolve parents for {source}"))?;

        // Fold make.defaults layers before inheriting other profile files so active
        // USE_EXPAND member variables use incremental semantics.
        let mut profile = Self::load(&source, Precedence::Profile(parents.len()))?;
        let layers = parents
            .iter()
            .map(|p| &p.make_defaults)
            .chain(iter::once(&profile.make_defaults))
            .collect::<Vec<_>>();
        profile.make_defaults = MakeEnv::fold_profile_defaults(&layers)?;
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
        let eapi = read_eapi(&path.join("eapi"))?;
        let recursive = eapi.supports_profile_file_dirs()
            || source.owning_repo.layout().supports_profile_file_dirs();

        let profile = Self {
            make_defaults: MakeEnv::from_path(&path.join("make.defaults"), false, true)?,
            deprecated: DeprecationInfo::from_path(&path.join("deprecated"))?,
            system_set: SystemPackages::from_path(&path.join("packages"), order)?,
            package_accept_keywords: PackageAcceptKeywords::from_path(
                &path.join("package.accept_keywords"),
                order,
                recursive,
            )?,
            package_mask: AtomEntries::from_path(&path.join("package.mask"), order, recursive)?,
            package_unmask: AtomEntries::from_path(&path.join("package.unmask"), order, recursive)?,
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
        self.system_set.inherit_from(&parent.system_set)?;
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
        reposet: &'repo RepoSet,
        profiles: &mut Vec<Self>,
        dfs: &mut DfsState<PathBuf>,
    ) -> anyhow::Result<()> {
        if let Err(cycle) = dfs.enter(&source.path) {
            let path = cycle.path.iter().map(|p| p.display()).format(" -> ");
            bail!("profile parent cycle detected involving {path}");
        }

        for parent in ParentEntry::from_parent_file(&source.path.join("parent"))? {
            let source = parent.resolve(source, reposet).with_context(|| {
                format!("invalid parent reference '{parent}' in profile {source}")
            })?;
            Self::build_parents(&source, reposet, profiles, dfs)?;

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
    use std::fs;
    use std::path::Path;

    use germ_pms::Atom;

    use super::*;
    use crate::files::entry::Precedence;
    use crate::files::pkgfile::KeywordRule;
    use crate::files::test_support::flags_for;
    use crate::profile::test_support::{
        ProfileBuilder, ProfileEntry, keyword_selectors, profile_entries, profile_entry,
        resolve_profile,
    };
    use crate::repository::test_support::{RepoBuilder, repo_set};
    use crate::test_support::assert_display_eq;

    fn rules_for(profile: &Profile, atom: &str) -> anyhow::Result<Vec<KeywordRule>> {
        let atom: Atom = atom.parse()?;
        Ok(profile
            .package_accept_keywords
            .clone()
            .into_rules()
            .filter_map(|(rule_atom, rule)| (rule_atom == atom).then_some(rule))
            .collect())
    }

    fn assert_parent_cases(format: &str) -> anyhow::Result<()> {
        let cross_repo = format == "portage-2";
        let reposet = repo_set(vec![
            RepoBuilder::new("source").formats([format]).profiles([
                ProfileBuilder::new("base"),
                ProfileBuilder::new("relative").parent("../base"),
                ProfileBuilder::new("cross-repo").parent("target:base"),
                ProfileBuilder::new("root-relative").parent(":base"),
            ]),
            RepoBuilder::new("target").formats(["pms"]).profile("base"),
        ])?;

        let cases = [
            ("relative", true),
            ("cross-repo", cross_repo),
            ("root-relative", cross_repo),
        ];
        for (profile, succeeds) in cases {
            assert_eq!(
                resolve_profile(&reposet, "source", profile).is_ok(),
                succeeds,
                "{format} {profile}"
            );
        }
        Ok(())
    }

    fn assert_package_accept_keywords(location: &Path, profile: &Profile) -> anyhow::Result<()> {
        let atom = "dev-lang/rust";
        let expected = keyword_selectors(
            location,
            "package.accept_keywords",
            &[
                ProfileEntry::new("base", "amd64", Precedence::Profile(0), 1),
                ProfileEntry::new("parent", "~amd64", Precedence::Profile(1), 1),
                ProfileEntry::new("selected", "**", Precedence::Profile(2), 1),
            ],
        );

        assert_eq!(rules_for(profile, atom)?, expected, "{atom}");
        Ok(())
    }

    fn assert_package_use_mask(location: &Path, profile: &Profile) -> anyhow::Result<()> {
        let package_use_mask = profile
            .use_records
            .package_use_mask
            .clone()
            .expand(&profile.use_records.expand_config)?;
        let flags = flags_for(&package_use_mask, "dev-lang/rust")?;

        let cases: &[(&str, Option<ProfileEntry<'_>>)] = &[
            (
                "llvm_targets_AMDGPU",
                Some(ProfileEntry::new(
                    "parent",
                    "llvm_targets_AMDGPU",
                    Precedence::Profile(1),
                    1,
                )),
            ),
            ("llvm_targets_X86", None),
            (
                "baz",
                Some(ProfileEntry::new(
                    "selected",
                    "baz",
                    Precedence::Profile(2),
                    1,
                )),
            ),
        ];

        for &(flag, expected) in cases {
            let expected = expected.map(|entry| profile_entry(location, "package.use.mask", entry));
            assert_eq!(flags.get(&flag.parse()?), expected.as_ref(), "{flag}");
        }
        Ok(())
    }

    #[test]
    fn test_profile_resolve() -> anyhow::Result<()> {
        let reposet = repo_set(vec![
            RepoBuilder::new("repo").formats(["pms"]).profiles([
                ProfileBuilder::new("base")
                    .file(
                        "make.defaults",
                        "USE_EXPAND=\"CAMERAS LLVM_TARGETS\"\nCAMERAS=\"canon ptp2\"\n",
                    )
                    .file("use.mask", "foo\nbar\n")
                    .file("package.accept_keywords", "dev-lang/rust amd64\n")
                    .file("package.use.mask", "dev-lang/rust LLVM_TARGETS: X86\n"),
                ProfileBuilder::new("parent")
                    .parent("../base")
                    .file("make.defaults", "CAMERAS=\"-canon nikon\"\n")
                    .file("use.mask", "-bar\nbaz\n")
                    .file("package.accept_keywords", "dev-lang/rust ~amd64\n")
                    .file(
                        "package.use.mask",
                        "dev-lang/rust LLVM_TARGETS: -* AMDGPU\n",
                    ),
                ProfileBuilder::new("selected")
                    .parent("../parent")
                    .file("use.mask", "-bar\nqux\n")
                    .file("package.accept_keywords", "dev-lang/rust **\n")
                    .file("package.use.mask", "dev-lang/rust baz\n"),
            ]),
        ])?;

        let repository = reposet.get("repo").unwrap();
        let profile = resolve_profile(&reposet, "repo", "selected")?;

        assert_eq!(
            profile.make_defaults.get("CAMERAS").unwrap().to_string(),
            "ptp2 nikon"
        );
        assert_package_accept_keywords(repository.location(), &profile)?;
        assert_package_use_mask(repository.location(), &profile)?;

        assert_eq!(
            profile
                .use_records
                .use_mask
                .into_entries()
                .collect::<Vec<_>>(),
            profile_entries(
                repository.location(),
                "use.mask",
                &[
                    ProfileEntry::new("base", "foo", Precedence::Profile(0), 1),
                    ProfileEntry::new("parent", "baz", Precedence::Profile(1), 2),
                    ProfileEntry::new("selected", "-bar", Precedence::Profile(2), 1),
                    ProfileEntry::new("selected", "qux", Precedence::Profile(2), 2),
                ],
            )
        );
        Ok(())
    }

    #[test]
    fn test_profile_file_dirs() -> anyhow::Result<()> {
        let cases = [
            ("pms", "0", "use.mask", "test\n", false),
            ("pms", "7", "use.mask", "test\n", true),
            ("portage-2", "8", "packages", "sys-apps/coreutils\n", false),
        ];

        for (format, eapi, file, contents, succeeds) in cases {
            let reposet = repo_set([RepoBuilder::new("repo").formats([format]).profile(
                ProfileBuilder::new("selected")
                    .eapi(eapi)
                    .entries_dir(file, contents),
            )])?;

            assert_eq!(
                resolve_profile(&reposet, "repo", "selected").is_ok(),
                succeeds,
                "{format} eapi {eapi} {file}"
            );
        }
        Ok(())
    }

    #[test]
    fn test_system_set_inherit() -> anyhow::Result<()> {
        let reposet = repo_set(vec![
            RepoBuilder::new("repo").formats(["portage-2"]).profiles([
                ProfileBuilder::new("base")
                    .file("packages", "*sys-apps/coreutils\n*sys-apps/busybox\n"),
                ProfileBuilder::new("selected")
                    .parent("../base")
                    .file("packages", "-*\n*sys-apps/busybox\n"),
            ]),
        ])?;

        let profile = resolve_profile(&reposet, "repo", "selected")?;

        assert_display_eq(profile.system_set.finalize(), ["sys-apps/busybox"]);
        Ok(())
    }

    #[test]
    fn test_parent_formats() -> anyhow::Result<()> {
        for format in ["pms", "portage-1", "portage-2"] {
            assert_parent_cases(format)?;
        }
        Ok(())
    }

    #[test]
    fn test_invalid_parents() -> anyhow::Result<()> {
        let reposet = repo_set([RepoBuilder::new("repo").formats(["portage-2"]).profiles([
            ProfileBuilder::new("root-relative").parent(":../outside"),
            ProfileBuilder::new("ordinary-relative").parent("../../outside"),
            ProfileBuilder::new("selected").parent("../selected"),
            ProfileBuilder::new("first").parent("../second"),
            ProfileBuilder::new("second").parent("../third"),
            ProfileBuilder::new("third").parent("../first"),
        ])])?;

        let repo_path = reposet.get("repo").unwrap().location();
        fs::create_dir(repo_path.join("outside"))?;

        for profile in ["root-relative", "ordinary-relative", "selected", "first"] {
            let result = resolve_profile(&reposet, "repo", profile);
            assert!(result.is_err(), "{profile}");
        }
        Ok(())
    }
}
