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
    pub repos: HashMap<String, Repository>,
}

impl RepoManager {
    /// Builds a [`RepoManager`] from the repos.conf configuration from the given `location`.
    pub fn new(location: &Path) -> Result<Self> {
        let config = RepoManagerConfig::load(location)?;
        let mut repos = HashMap::with_capacity(config.repo_confs.len());

        // config::load() already sorts repositories based on their masters,
        // so we can just iterate through them in order and populate their packages and metadata.
        for repo_conf in &config.repo_confs {
            let mut repo = Repository::new(repo_conf)?;
            // TODO: find a better way to resolve this without allocations
            let possible_masters = repos.values().collect::<Vec<_>>();
            let masters =
                Self::resolve_masters(repo.masters.clone(), &possible_masters).collect::<Vec<_>>();
            repo.populate(&masters)?;
            debug!(
                "Loaded repository '{}' with {} packages",
                repo.name,
                repo.packages.len()
            );
            repos.insert(repo.name.clone(), repo);
        }

        Ok(Self { repos })
    }

    /// Resolves the ebuild for the given `package` by searching through all repositories.
    /// Returns Err if the package or repository doesn't exist, or if the ebuild cannot be
    /// resolved for any reason.
    pub fn resolve_ebuild<'a>(&'a self, package: &'a Package) -> Result<Ebuild<'a>> {
        let repo = self
            .repos
            .get(&package.repo)
            .ok_or_else(|| anyhow!("repository {} doesn't exist", package.name))?;

        if !repo.packages.contains(package) {
            return Err(anyhow!("unable to find {package} in {repo}"));
        }

        repo.resolve_ebuild(package)
            .with_context(|| anyhow!("unable to resolve ebuild for {package}"))
    }

    /// Helper function to recursively resolve all master repositories for a given `repo` and return
    /// an `Iterator` over them.
    ///
    /// NOTE: If a repository is listed as a master but doesn't exist, it will be silently ignored.
    fn resolve_masters<'a>(
        masters: Vec<String>,
        repos: &'a [&Repository],
    ) -> Box<dyn Iterator<Item = &'a Repository> + 'a> {
        let iter = masters
            .into_iter()
            .filter_map(|name| repos.iter().find(|r| r.name == *name))
            .flat_map(|repo| {
                iter::once(*repo).chain(Self::resolve_masters(repo.masters.clone(), repos))
            });
        Box::new(iter)
    }
}

impl fmt::Display for RepoManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (name, repo) in &self.repos {
            writeln!(f, "{name}\n    location: {}\n", repo.location.display())?;
        }
        Ok(())
    }
}
