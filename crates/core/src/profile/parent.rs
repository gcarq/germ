use super::ProfileSource;
use crate::repository::RepoSet;
use anyhow::{Context, Result, anyhow, bail};
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

/// Represents a parsed entry from a profile's `parent` file.
/// This can be in multiple formats, depending on the profile format.
pub enum ParentEntry {
    // format: <path>
    RelativePath(PathBuf),
    // format: :<path>
    RootRelative(PathBuf),
    // format: <repository>:<path>
    CrossRepository {
        repo_name: String,
        profile_path: PathBuf,
    },
}

impl ParentEntry {
    /// Reads the parent file at the given `path` and returns its entries.
    ///
    /// Returns an empty vec if `path` doesn't exist.
    pub fn from_parent_file(path: &Path) -> Result<Vec<Self>> {
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(path)
            .with_context(|| anyhow!("unable to read parent file {}", path.display()))?;
        content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(Self::try_from)
            .collect::<Result<_>>()
            .with_context(|| anyhow!("unable to parse parent file {}", path.display()))
    }

    /// Resolves a parent [`ProfileSource`] with the canonical path and repository for the given `referring_profile`.
    ///
    /// Returns `Err` if the path is invalid, the repository is not available, or the parent profile isn't within the repository.
    pub fn resolve<'repo>(
        &self,
        referring_profile: &ProfileSource<'repo>,
        repo_set: &'repo RepoSet,
    ) -> Result<ProfileSource<'repo>> {
        let owning_repo = referring_profile.owning_repo;
        match self {
            Self::RelativePath(profile_path) => {
                if !profile_path.is_relative() {
                    bail!("parent path '{}' must be relative", profile_path.display());
                }
                let path = referring_profile
                    .path
                    .join(profile_path)
                    .canonicalize()
                    .with_context(|| {
                        anyhow!("unable to resolve parent path '{}'", profile_path.display())
                    })?;
                let canonical_root = owning_repo.location.join("profiles").canonicalize()?;
                if !path.starts_with(&canonical_root) {
                    bail!(
                        "parent profile {} escapes repository profiles root {}",
                        path.display(),
                        canonical_root.display()
                    );
                }
                Ok(ProfileSource { path, owning_repo })
            }
            Self::RootRelative(profile_path) => {
                if !owning_repo.layout.supports_root_relative_parents() {
                    bail!("root-relative parent requires profile-format 'portage-2'");
                }
                let path =
                    Self::resolve_contained(&owning_repo.location.join("profiles"), profile_path)?;
                Ok(ProfileSource {
                    path,
                    owning_repo: referring_profile.owning_repo,
                })
            }
            Self::CrossRepository {
                repo_name,
                profile_path,
            } => {
                if !owning_repo.layout.supports_cross_repo_parents() {
                    bail!("cross-repository parent requires profile-format 'portage-2'");
                }
                let parent_repo = repo_set.get(repo_name).ok_or_else(|| {
                    anyhow!("repository '{repo_name}' is not available for parent '{self}'")
                })?;
                let path =
                    Self::resolve_contained(&parent_repo.location.join("profiles"), profile_path)?;
                Ok(ProfileSource {
                    path,
                    owning_repo: parent_repo,
                })
            }
        }
    }

    /// Resolves the given `profile_path` relative to the `profiles_root` and ensures that it doesn't escape it.
    fn resolve_contained(profiles_root: &Path, profile_path: &Path) -> Result<PathBuf> {
        if profile_path.as_os_str().is_empty() {
            bail!("parent path must not be empty");
        }
        if !profile_path.is_relative() {
            bail!("parent path '{}' must be relative", profile_path.display());
        }
        let canonical_root = profiles_root.canonicalize().with_context(|| {
            anyhow!(
                "unable to resolve repository profiles root {}",
                profiles_root.display()
            )
        })?;
        let target = profiles_root.join(profile_path);
        let canonical_target = target
            .canonicalize()
            .with_context(|| anyhow!("unable to resolve parent profile {}", target.display()))?;
        if !canonical_target.starts_with(&canonical_root) {
            bail!(
                "parent profile {} escapes repository profiles root {}",
                canonical_target.display(),
                canonical_root.display()
            );
        }
        Ok(canonical_target)
    }
}

impl TryFrom<&str> for ParentEntry {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if let Some(profile_path) = value.strip_prefix(':') {
            return Ok(Self::RootRelative(PathBuf::from(profile_path)));
        }

        if let Some((repo_name, profile_path)) = value.split_once(':') {
            return Ok(Self::CrossRepository {
                repo_name: repo_name.to_owned(),
                profile_path: PathBuf::from(profile_path),
            });
        }

        Ok(Self::RelativePath(PathBuf::from(value)))
    }
}

impl fmt::Display for ParentEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelativePath(profile_path) => profile_path.display().fmt(f),
            Self::RootRelative(profile_path) => write!(f, ":{}", profile_path.display()),
            Self::CrossRepository {
                repo_name,
                profile_path,
            } => write!(f, "{repo_name}:{}", profile_path.display()),
        }
    }
}
