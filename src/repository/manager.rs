use crate::conf::PortageConf;
use crate::ebuild::Ebuild;
use crate::package::Package;
use crate::repository::Repository;
use crate::repository::config::RepoManagerConfig;
use crate::repository::eclass::Eclass;
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
    pub fn get_repo(&self, name: &str) -> Option<&Repository> {
        self.repositories.get(name)
    }

    /// Returns the main repository.
    pub fn main_repo(&self) -> &Repository {
        self.repositories
            .get(&self.main_repo_name)
            .expect("FATAL: no main repository assigned")
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
            .repositories
            .get(&package.repo)
            .ok_or_else(|| anyhow!("repository {} doesn't exist", package.name))?;

        if !repo.packages.contains(package) {
            return Err(anyhow!("unable to find {package} in {repo}"));
        }

        repo.resolve_ebuild(package)
            .with_context(|| anyhow!("unable to resolve ebuild for {package}"))
    }

    /// Resolves an eclass with the given `eclass_name` and `repo_name`
    /// that should be used for the search.
    pub fn resolve_eclass(&self, eclass_name: &str, repo_name: &str) -> Result<&Eclass> {
        let repo = self
            .repositories
            .get(repo_name)
            .ok_or_else(|| anyhow!("repository '{repo_name}' doesn't exist"))?;

        let eclass = repo
            .resolve_masters(self)
            .find_map(|repo| repo.eclasses.get(eclass_name))
            .ok_or_else(|| {
                anyhow!("eclass '{eclass_name}' not found in repository '{repo}' or its masters")
            })?;

        Ok(eclass)
    }

    /// Generates metadata for either the given `repo_name` or all repositories.
    pub fn generate_metadata(&self, conf: &PortageConf, repo_name: Option<String>) -> Result<()> {
        if let Some(repo) = repo_name {
            let repo = self
                .repositories
                .get(&repo)
                .ok_or_else(|| anyhow!("repository '{repo}' doesn't exist"))?;
            return repo.generate_metadata(conf);
        }

        for repo in self.repositories() {
            repo.generate_metadata(conf)?;
        }
        Ok(())
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
