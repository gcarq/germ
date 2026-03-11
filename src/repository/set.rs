use crate::deps::atom::Atom;
use crate::package::Package;
use crate::repository::Repository;
use crate::repository::config::{RepoSetConfig, RepositoryConfig};
use crate::utils::Inherit;
use anyhow::{Context, Result, anyhow};
use log::debug;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::{fmt, iter};

/// Resolves and handles all available [`Repository`] instances.
///
/// It gets configured via `repos.conf` that usually resides in `/etc/portage/`.
/// See <https://dev.gentoo.org/~zmedico/portage/doc/man/portage.5.html>
#[cfg_attr(test, derive(Default))]
pub struct RepoSet {
    repos: HashMap<String, Repository>,
}

impl RepoSet {
    /// Builds a [`RepoSet`] from the repos.conf configuration from the given `location`.
    pub fn new(location: &Path) -> Result<Self> {
        let config = RepoSetConfig::load(location)?;
        let mut repos = HashMap::with_capacity(config.repo_confs.len());

        // config::load() already sorts repositories based on their masters,
        // so we can just iterate through them in order and populate their packages and metadata.
        for repo_conf in &config.repo_confs {
            let mut repo = Repository::new(repo_conf)?;
            for master in Self::resolve_masters(repo_conf, &config, &repos) {
                repo.inherit_from(master);
            }
            repo.populate()?;

            debug!(
                "Loaded repository '{}' with {} packages",
                repo.name,
                repo.avail_package_idx.len()
            );
            repo.load_index()
                .with_context(|| anyhow!("unable to load package index for {repo}"))?;
            repos.insert(repo.name.clone(), repo);
        }

        Ok(Self { repos })
    }

    /// Finds and returns all packages that match the given `atom`.
    /// TODO: Order the returned packages by version
    /// TODO: Consider returning an iterator
    pub fn find_packages(&mut self, atom: &Atom) -> Result<Vec<&Package>> {
        if let Some(repo_name) = &atom.repo {
            let repo = self
                .repos
                .get_mut(repo_name)
                .ok_or_else(|| anyhow!("repository '{repo_name}' not found for atom '{atom}'"))?;
            return repo.find_packages(atom);
        }

        let mut pkgs = Vec::new();
        for repo in self.repos.values_mut() {
            pkgs.extend(repo.find_packages(atom)?);
        }
        Ok(pkgs)
    }

    /// Force all repositories to write their indexes and caches to disk.
    pub fn flush(&self) -> Result<()> {
        for repo in self.repos.values() {
            repo.write_index()?;
        }
        Ok(())
    }

    /// Helper function to recursively resolve all master repositories for a given `conf` and return
    /// an `Iterator` over them.
    ///
    /// NOTE: If a repository is listed as a master but doesn't exist, it will be silently ignored.
    fn resolve_masters<'a>(
        conf: &'a RepositoryConfig,
        set_conf: &'a RepoSetConfig,
        repos: &'a HashMap<String, Repository>,
    ) -> Box<dyn Iterator<Item = &'a Repository> + 'a> {
        let iter = conf
            .masters
            .iter()
            .filter_map(|name| repos.get(name))
            .flat_map(|repo| {
                let repo_conf = set_conf
                    .get(&repo.name)
                    .expect("config should exist since repo is already loaded");
                iter::once(repo).chain(Self::resolve_masters(repo_conf, set_conf, repos))
            });
        Box::new(iter)
    }
}

impl Deref for RepoSet {
    type Target = HashMap<String, Repository>;
    fn deref(&self) -> &Self::Target {
        &self.repos
    }
}

impl DerefMut for RepoSet {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.repos
    }
}

impl fmt::Display for RepoSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (name, repo) in &self.repos {
            writeln!(f, "{name}\n    location: {}\n", repo.location.display())?;
        }
        Ok(())
    }
}
