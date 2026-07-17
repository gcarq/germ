use crate::deps::atom::Atom;
use crate::package::Package;
use crate::repository::Repository;
use crate::repository::config::{RepoSetConfig, RepositoryConfig};
use crate::types::FxHashMap;
use crate::utils::Inherit;
use anyhow::{Context, Result, anyhow, bail};
use log::{debug, error, warn};
use std::ops::{Deref, DerefMut};
use std::path::Path;

/// Resolves and handles all available [`Repository`] instances.
///
/// It gets configured via `repos.conf` that usually resides in `/etc/portage/`.
/// See <https://dev.gentoo.org/~zmedico/portage/doc/man/portage.5.html>
#[cfg_attr(test, derive(Default, Debug))]
pub struct RepoSet {
    config: RepoSetConfig,
    repos: FxHashMap<String, Repository>,
}

/// Defines a visiting state for resolving inheritance with DFS.
enum VisitState {
    Visiting,
    Done,
}

impl RepoSet {
    /// Builds a [`RepoSet`] from the repos.conf configuration from the given `location`.
    pub fn new(location: &Path) -> Result<Self> {
        let mut set = Self {
            config: RepoSetConfig::load(location)?,
            repos: FxHashMap::default(),
        };
        set.reload_from_disk()
            .with_context(|| "unable to inizialize local data for repositories")?;
        Ok(set)
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

    /// Syncs all repositories and reloads repository data from disk after successful syncs.
    ///
    /// If a sync fails, an error is logged
    pub fn maybe_sync(&mut self) -> Result<()> {
        for repo in self.repos.values() {
            if let Err(err) = repo.sync() {
                error!("Failed to sync repository '{}': {}", repo.name, err);
            }
        }

        self.reload_from_disk()
            .with_context(|| "unable to reload local data for repositories")
    }

    /// Write repository indexes and caches to disk.
    pub fn flush(&self, force: bool) -> Result<()> {
        for repo in self.repos.values() {
            if repo.is_loaded() {
                repo.write_index(force)?;
            }
        }
        Ok(())
    }

    /// Reloads all repository data from disk.
    fn reload_from_disk(&mut self) -> Result<()> {
        let configs = self
            .config
            .iter()
            .map(|conf| (conf.name.clone(), conf.clone()))
            .collect::<FxHashMap<_, _>>();

        let mut repos = self
            .config
            .iter()
            .map(|conf| {
                let mut repo = Repository::new(conf)?;
                if repo.location.exists()
                    && let Err(err) = repo.load_data_from_disk()
                {
                    // The main repository must exist and be correct
                    if repo.name == self.config.main_repo_name {
                        return Err(err).with_context(|| {
                            format!("Unable to load data for main repository '{}'", repo.name)
                        });
                    }
                    warn!("Unable to load data for repository '{}': {err}", repo.name);
                }
                Ok((repo.name.clone(), repo))
            })
            .collect::<Result<FxHashMap<_, _>>>()?;

        let mut states = FxHashMap::default();
        let names = repos.keys().cloned().collect::<Vec<_>>();
        for name in names {
            if repos.get(&name).is_some_and(Repository::is_loaded) {
                Self::finalize_repo_with_masters(&name, &mut repos, &configs, &mut states)?;
                debug!(
                    "Loaded repository '{name}' with {} ebuilds",
                    repos.get(&name).unwrap().cpvs()?.count()
                );
            }
        }

        self.repos = repos;
        Ok(())
    }

    /// Finalizes the given `repo_name` after recursively finalizing its loaded masters.
    fn finalize_repo_with_masters(
        repo_name: &str,
        repos: &mut FxHashMap<String, Repository>,
        configs: &FxHashMap<String, RepositoryConfig>,
        states: &mut FxHashMap<String, VisitState>,
    ) -> Result<()> {
        match states.get(repo_name) {
            Some(VisitState::Done) => return Ok(()),
            Some(VisitState::Visiting) => {
                bail!("Repository master cycle detected involving '{repo_name}'");
            }
            None => {}
        }

        states.insert(repo_name.to_owned(), VisitState::Visiting);

        let mut finalized_master_names = Vec::new();
        for master_name in Self::resolve_effective_masters(repo_name, repos, configs)? {
            if !repos.get(&master_name).is_some_and(Repository::is_loaded) {
                debug!("Skipping missing or unloaded master '{master_name}' for '{repo_name}'");
                continue;
            }

            Self::finalize_repo_with_masters(&master_name, repos, configs, states)?;
            finalized_master_names.push(master_name);
        }

        Self::finalize_loaded_repo(repo_name, repos, &finalized_master_names)?;
        states.insert(repo_name.to_owned(), VisitState::Done);
        Ok(())
    }

    /// Applies inheritance, populates repository data and loads indexes for the given repository.
    fn finalize_loaded_repo(
        name: &str,
        repos: &mut FxHashMap<String, Repository>,
        finalized_master_names: &[String],
    ) -> Result<()> {
        let mut repo = repos
            .remove(name)
            .with_context(|| anyhow!("repository '{name}' should exist while finalizing"))?;
        for master_name in finalized_master_names {
            let master = repos.get(master_name).with_context(|| {
                anyhow!("finalized master '{master_name}' should exist while finalizing '{name}'")
            })?;
            repo.inherit_from(master);
        }

        repo.populate()
            .with_context(|| anyhow!("unable to populate data for '{repo}' repository"))?;
        repo.load_index()
            .with_context(|| anyhow!("unable to load package index for '{repo}' repository"))?;

        repos.insert(repo.name.clone(), repo);
        Ok(())
    }

    /// Returns the effective masters for the given `repo_name`.
    ///
    /// masters from repos.conf take precedence over masters from layout.conf.
    fn resolve_effective_masters(
        repo_name: &str,
        repos: &FxHashMap<String, Repository>,
        configs: &FxHashMap<String, RepositoryConfig>,
    ) -> Result<Vec<String>> {
        let repo_conf = configs
            .get(repo_name)
            .ok_or_else(|| anyhow!("config for repository '{repo_name}' not found"))?;
        if let Some(masters) = &repo_conf.masters {
            return Ok(masters.clone());
        }

        let repo = repos
            .get(repo_name)
            .ok_or_else(|| anyhow!("repository '{repo_name}' not found"))?;
        if repo.is_loaded() {
            return Ok(repo.layout()?.masters.clone());
        }

        Ok(Vec::new())
    }
}

impl Deref for RepoSet {
    type Target = FxHashMap<String, Repository>;
    fn deref(&self) -> &Self::Target {
        &self.repos
    }
}

impl DerefMut for RepoSet {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.repos
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::test_support::RepositoryFixture;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn add_ebuild(repo: &Path, category: &str, package: &str, version: &str) -> Result<()> {
        let package_dir = repo.join(category).join(package);
        fs::create_dir_all(&package_dir)?;
        fs::write(package_dir.join(format!("{package}-{version}.ebuild")), "")?;
        Ok(())
    }

    fn write_repos_conf(root: &Path, content: &str) -> Result<PathBuf> {
        let path = root.join("repos.conf");
        fs::write(&path, content)?;
        Ok(path)
    }

    #[test]
    fn test_layout_masters_are_used() -> Result<()> {
        let temp = tempfile::Builder::new().prefix("pkgrove-test-").tempdir()?;
        let master = RepositoryFixture::new(temp.path().join("master"), "master")
            .categories(["app-misc"])
            .eclass("master")
            .write()?;
        let overlay = RepositoryFixture::new(temp.path().join("overlay"), "overlay")
            .masters(["master"])
            .write()?;
        add_ebuild(&overlay, "app-misc", "foo", "1")?;
        let repos_conf = write_repos_conf(
            temp.path(),
            &format!(
                "[master]\nlocation = {}\n\n[overlay]\nlocation = {}\n",
                master.display(),
                overlay.display()
            ),
        )?;

        let repo_set = RepoSet::new(&repos_conf)?;
        let has_package = repo_set
            .get("overlay")
            .unwrap()
            .cpvs()?
            .any(|cpv| cpv.fqn() == "app-misc/foo-1");

        assert!(has_package);
        assert!(
            repo_set
                .get("overlay")
                .unwrap()
                .eclasses()?
                .contains_key("master")
        );
        Ok(())
    }

    #[test]
    fn test_explicit_empty_masters_overrides_layout_masters() -> Result<()> {
        let temp = tempfile::Builder::new().prefix("pkgrove-test-").tempdir()?;
        let master = RepositoryFixture::new(temp.path().join("master"), "master")
            .categories(["app-misc"])
            .eclass("master")
            .write()?;
        let overlay = RepositoryFixture::new(temp.path().join("overlay"), "overlay")
            .masters(["master"])
            .write()?;
        add_ebuild(&overlay, "app-misc", "foo", "1")?;
        let repos_conf = write_repos_conf(
            temp.path(),
            &format!(
                "[master]\nlocation = {}\n\n[overlay]\nlocation = {}\nmasters =\n",
                master.display(),
                overlay.display()
            ),
        )?;

        let repo_set = RepoSet::new(&repos_conf)?;
        let has_package = repo_set
            .get("overlay")
            .unwrap()
            .cpvs()?
            .any(|cpv| cpv.fqn() == "app-misc/foo-1");

        assert!(!has_package);
        assert!(
            !repo_set
                .get("overlay")
                .unwrap()
                .eclasses()?
                .contains_key("master")
        );
        Ok(())
    }

    #[test]
    fn test_reload_from_disk_preserves_inherited_data() -> Result<()> {
        let temp = tempfile::Builder::new().prefix("pkgrove-test-").tempdir()?;
        let master = RepositoryFixture::new(temp.path().join("master"), "master")
            .categories(["app-misc"])
            .eclass("master")
            .write()?;
        let overlay = RepositoryFixture::new(temp.path().join("overlay"), "overlay")
            .masters(["master"])
            .write()?;
        add_ebuild(&overlay, "app-misc", "foo", "1")?;
        let repos_conf = write_repos_conf(
            temp.path(),
            &format!(
                "[master]\nlocation = {}\n\n[overlay]\nlocation = {}\n",
                master.display(),
                overlay.display()
            ),
        )?;

        let mut repo_set = RepoSet::new(&repos_conf)?;
        assert!(
            repo_set
                .get("overlay")
                .unwrap()
                .cpvs()?
                .any(|cpv| cpv.fqn() == "app-misc/foo-1")
        );

        repo_set.reload_from_disk()?;

        assert!(
            repo_set
                .get("overlay")
                .unwrap()
                .cpvs()?
                .any(|cpv| cpv.fqn() == "app-misc/foo-1")
        );
        assert!(
            repo_set
                .get("overlay")
                .unwrap()
                .eclasses()?
                .contains_key("master")
        );
        Ok(())
    }

    #[test]
    fn test_reload_refreshes_dependent_overlays() -> Result<()> {
        let temp = tempfile::Builder::new().prefix("pkgrove-test-").tempdir()?;
        let master = RepositoryFixture::new(temp.path().join("master"), "master")
            .categories(["app-misc"])
            .write()?;
        let overlay = RepositoryFixture::new(temp.path().join("overlay"), "overlay")
            .masters(["master"])
            .write()?;
        add_ebuild(&overlay, "app-misc", "foo", "1")?;
        add_ebuild(&overlay, "dev-libs", "bar", "1")?;
        let repos_conf = write_repos_conf(
            temp.path(),
            &format!(
                "[master]\nlocation = {}\n\n[overlay]\nlocation = {}\n",
                master.display(),
                overlay.display()
            ),
        )?;

        let mut repo_set = RepoSet::new(&repos_conf)?;
        assert!(
            !repo_set
                .get("overlay")
                .unwrap()
                .cpvs()?
                .any(|cpv| cpv.fqn() == "dev-libs/bar-1")
        );

        fs::write(
            master.join("profiles").join("categories"),
            "app-misc\ndev-libs\n",
        )?;
        repo_set.reload_from_disk()?;

        assert!(
            repo_set
                .get("overlay")
                .unwrap()
                .cpvs()?
                .any(|cpv| cpv.fqn() == "dev-libs/bar-1")
        );
        Ok(())
    }

    #[test]
    fn test_master_cycle_err() -> Result<()> {
        let temp = tempfile::Builder::new().prefix("pkgrove-test-").tempdir()?;
        let first = RepositoryFixture::new(temp.path().join("first"), "first")
            .masters(["second"])
            .write()?;
        let second = RepositoryFixture::new(temp.path().join("second"), "second")
            .masters(["first"])
            .write()?;
        let repos_conf = write_repos_conf(
            temp.path(),
            &format!(
                "[first]\nlocation = {}\n\n[second]\nlocation = {}\n",
                first.display(),
                second.display()
            ),
        )?;

        assert!(RepoSet::new(&repos_conf).is_err());
        Ok(())
    }

    #[test]
    fn test_missing_and_unloaded_masters_are_skipped() -> Result<()> {
        let temp = tempfile::Builder::new().prefix("pkgrove-test-").tempdir()?;
        let loaded = RepositoryFixture::new(temp.path().join("loaded"), "loaded")
            .categories(["app-misc"])
            .eclass("loaded")
            .write()?;
        let child = RepositoryFixture::new(temp.path().join("child"), "child").write()?;
        add_ebuild(&child, "app-misc", "foo", "1")?;
        let unavailable = temp.path().join("unavailable");
        let repos_conf = write_repos_conf(
            temp.path(),
            &format!(
                "[loaded]\nlocation = {}\n\n[unavailable]\nlocation = {}\nsync-type = git\nsync-uri = https://example.invalid/unavailable.git\n\n[child]\nlocation = {}\nmasters = missing loaded unavailable\n",
                loaded.display(),
                unavailable.display(),
                child.display()
            ),
        )?;

        let repo_set = RepoSet::new(&repos_conf)?;
        let child = repo_set.get("child").unwrap();

        assert!(!repo_set.get("unavailable").unwrap().is_loaded());
        assert!(child.eclasses()?.contains_key("loaded"));
        assert!(child.cpvs()?.any(|cpv| cpv.fqn() == "app-misc/foo-1"));
        Ok(())
    }

    #[test]
    fn test_direct_master_order_is_preserved() -> Result<()> {
        let temp = tempfile::Builder::new().prefix("pkgrove-test-").tempdir()?;
        let first = RepositoryFixture::new(temp.path().join("first"), "first")
            .eclass("shared")
            .write()?;
        let second = RepositoryFixture::new(temp.path().join("second"), "second")
            .eclass("shared")
            .write()?;
        let child = RepositoryFixture::new(temp.path().join("child"), "child").write()?;
        let repos_conf = write_repos_conf(
            temp.path(),
            &format!(
                "[first]\nlocation = {}\n\n[second]\nlocation = {}\n\n[child]\nlocation = {}\nmasters = first second\n",
                first.display(),
                second.display(),
                child.display()
            ),
        )?;

        let repo_set = RepoSet::new(&repos_conf)?;
        let shared = repo_set
            .get("child")
            .unwrap()
            .eclasses()?
            .get("shared")
            .unwrap();

        assert!(shared.path.starts_with(second));
        Ok(())
    }
}
