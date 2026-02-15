use crate::ebuild::Ebuild;
use crate::package::Package;
use crate::repository::Repository;
use crate::repository::config::RepoManagerConfig;
use anyhow::{Context, Result, anyhow};
use log::debug;
use std::collections::HashMap;
use std::path::Path;
use std::{fmt, iter};

/// Resolves and handles all available [`Repository`] instances.
///
/// It gets configured via `repos.conf` that usually resides in `/etc/portage/`.
/// See <https://dev.gentoo.org/~zmedico/portage/doc/man/portage.5.html>
pub struct RepoManager {
    main_repo_name: String,
    repositories: HashMap<String, Repository>,
}

impl RepoManager {
    /// Builds a [`RepoManager`] from the repos.conf configuration from the given `location`.
    pub fn new(location: &Path) -> Result<Self> {
        let config = RepoManagerConfig::load(location)?;
        let mut repositories = HashMap::with_capacity(config.repo_confs.len());

        // config::load() already sorts repositories based on their masters,
        // so we can just iterate through them in order and populate their packages and metadata.
        for repo_conf in &config.repo_confs {
            let mut repo = Repository::new(repo_conf)?;
            let masters = repositories
                .iter()
                .filter(|(name, _)| repo.masters.contains(name))
                .map(|(_, repo)| repo)
                .collect::<Vec<_>>();
            repo.populate(&masters)?;
            debug!(
                "Loaded repository '{}' with {} packages",
                repo.name,
                repo.packages.len()
            );
            repositories.insert(repo.name.clone(), repo);
        }

        Ok(Self {
            main_repo_name: config.main_repo_name,
            repositories,
        })
    }

    /// Returns the repository with the given `name` if it exists.
    pub fn get(&self, name: &str) -> Option<&Repository> {
        self.repositories.get(name)
    }

    /// Returns the main repository.
    pub fn main_repo(&self) -> &Repository {
        self.get(&self.main_repo_name)
            .expect("FATAL: no main repository assigned")
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Repository)> {
        self.repositories.iter()
    }

    /// Returns an `Iterator` over all repositories, with the main repository first.
    pub fn repositories(&self) -> impl Iterator<Item = &Repository> {
        iter::once(self.main_repo()).chain(self.repositories.iter().filter_map(|(name, repo)| {
            if name != &self.main_repo_name {
                Some(repo)
            } else {
                None
            }
        }))
    }

    /// Resolves the ebuild for the given `package` by searching through all repositories.
    /// Returns Err if the package or repository doesn't exist, or if the ebuild cannot be
    /// resolved for any reason.
    pub fn resolve_ebuild<'a>(&self, package: &'a Package) -> Result<Ebuild<'a>> {
        let repo = self
            .get(&package.repo)
            .ok_or_else(|| anyhow!("repository {} doesn't exist", package.name))?;

        if !repo.packages.contains(package) {
            return Err(anyhow!("unable to find {package} in {repo}"));
        }

        repo.resolve_ebuild(package)
            .with_context(|| anyhow!("unable to resolve ebuild for {package}"))
    }
}

/// This default implementation should only be used for testing.
impl Default for RepoManager {
    fn default() -> Self {
        Self {
            main_repo_name: "gentoo".to_string(),
            repositories: HashMap::new(),
        }
    }
}

impl fmt::Display for RepoManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (name, repo) in &self.repositories {
            writeln!(f, "{name}\n    location: {}\n", repo.location.display())?;
        }
        Ok(())
    }
}
