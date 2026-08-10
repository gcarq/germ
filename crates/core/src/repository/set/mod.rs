mod config;
mod error;
mod sync;

use self::config::RepoSetConfig;
pub use self::error::RepoSetError;
use self::sync::{SyncHandler, build_sync_handler};
use super::tree::{Repository, RepositoryError};
use crate::deps::atom::Atom;
use crate::repository::tree::PackageResult;
use crate::types::{FxHashMap, FxHashSet};
use crate::utils::Inherit;
use anyhow::anyhow;
use either::Either;
use log::{debug, error, info, warn};
use std::{fs, io, path::Path};

/// Resolves and handles all available [`Repository`] instances.
///
/// It gets configured via `repos.conf` that usually resides in `/etc/portage/`.
/// See <https://dev.gentoo.org/~zmedico/portage/doc/man/portage.5.html>
#[cfg_attr(test, derive(Default, Debug))]
pub struct RepoSet {
    config: RepoSetConfig,
    entries: FxHashMap<String, RepositoryEntry>,
}

/// Holds the result and sync handler of a configured repository.
/// If a repository doesn't exist locally it will be `None` here, but
/// can be synced with the [`SyncHandler`].
#[derive(Debug)]
struct RepositoryEntry {
    repository: Option<Repository>,
    sync_handler: Option<Box<dyn SyncHandler>>,
}

impl RepositoryEntry {
    fn sync(&self) -> anyhow::Result<()> {
        if let Some(handler) = &self.sync_handler {
            handler.sync()?;
        }
        Ok(())
    }
}

impl RepoSet {
    /// Builds a [`RepoSet`] from the repos.conf configuration from the given `location`.
    pub fn new(location: &Path) -> Result<Self, RepoSetError> {
        let config = RepoSetConfig::load(location).map_err(|error| {
            RepoSetError::Configuration(error.context(format!(
                "unable to load repository configuration from {}",
                location.display()
            )))
        })?;
        let mut set = Self {
            config,
            entries: FxHashMap::default(),
        };
        set.reload_from_disk()?;
        Ok(set)
    }

    /// Eagerly resolves and returns all packages that match the given `atom`.
    /// TODO: Order the returned packages by version
    pub fn find_packages<'r>(
        &'r self,
        atom: &Atom,
    ) -> Result<Vec<PackageResult<'r>>, RepoSetError> {
        let mut results = Vec::new();
        for repo in self.select(atom.repo.as_deref()) {
            results.extend(
                repo.find_packages(atom)
                    .map_err(|err| RepoSetError::repo_failure(&repo.name, err))?,
            );
        }
        Ok(results)
    }

    /// Attempts to synchronize all repositories and reloads repo data from disk.
    ///
    /// A sync failure is logged as error but doesn't return an `Err`.
    pub fn maybe_sync(&mut self) -> Result<(), RepoSetError> {
        for (name, entry) in &self.entries {
            info!("Syncing repository '{name}'");
            if let Err(err) = entry.sync() {
                error!("Failed to sync repository '{name}': {err}");
            }
        }

        self.reload_from_disk()
    }

    pub fn get(&self, name: &str) -> Option<&Repository> {
        self.entries.get(name)?.repository.as_ref()
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Repository> {
        self.entries.get_mut(name)?.repository.as_mut()
    }

    /// Returns an iterator over all repositories, or just the repository with the given `name`.
    pub fn select(&self, name: Option<&str>) -> impl Iterator<Item = &Repository> {
        match name {
            Some(name) => Either::Left(self.get(name).into_iter()),
            None => Either::Right(self.values()),
        }
    }

    /// Returns a mutable iterator over all repositories, or just the repository with the given `name`.
    pub fn select_mut(&mut self, name: Option<&str>) -> impl Iterator<Item = &mut Repository> {
        match name {
            Some(name) => Either::Left(self.get_mut(name).into_iter()),
            None => Either::Right(self.values_mut()),
        }
    }

    pub fn values(&self) -> impl Iterator<Item = &Repository> {
        self.entries
            .values()
            .filter_map(|entry| entry.repository.as_ref())
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut Repository> {
        self.entries
            .values_mut()
            .filter_map(|entry| entry.repository.as_mut())
    }

    pub fn drain(self) -> impl Iterator<Item = Repository> {
        self.entries
            .into_values()
            .filter_map(|entry| entry.repository)
    }

    pub fn len(&self) -> usize {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.repository.is_some())
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Reloads all repository data from disk.
    pub(crate) fn reload_from_disk(&mut self) -> Result<(), RepoSetError> {
        let mut entries = FxHashMap::default();
        for config in self.config.iter() {
            let sync_handler = build_sync_handler(&config.raw_properties).map_err(|error| {
                RepoSetError::Configuration(
                    error.context(format!("unable to configure repository '{}'", config.name)),
                )
            })?;
            entries.insert(
                config.name.clone(),
                RepositoryEntry {
                    repository: None,
                    sync_handler,
                },
            );
        }

        self.entries = entries;

        let mut pending = FxHashMap::default();
        for config in self.config.iter() {
            match fs::metadata(&config.location) {
                Ok(_) => match Repository::load(&config.name, &config.location) {
                    Ok(repository) => {
                        pending.insert(config.name.clone(), repository);
                    }
                    Err(
                        error @ (RepositoryError::Data(_)
                        | RepositoryError::Layout(_)
                        | RepositoryError::Profile(_)),
                    ) => {
                        warn!("Repository '{}' is unavailable: {error:#}", config.name);
                    }
                    Err(source) => {
                        return Err(RepoSetError::repo_failure(&config.name, source));
                    }
                },
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(RepoSetError::Configuration(
                        anyhow::Error::new(error).context(format!(
                            "unable to inspect location {} for repository '{}'",
                            config.location.display(),
                            config.name
                        )),
                    ));
                }
            }
        }

        let graph = self
            .config
            .iter()
            .map(|config| {
                let masters = config.masters.clone().unwrap_or_else(|| {
                    pending
                        .get(&config.name)
                        .map(|repository| repository.layout.masters.clone())
                        .unwrap_or_default()
                });
                (config.name.clone(), masters)
            })
            .collect::<FxHashMap<_, _>>();
        Self::validate_master_graph(&graph)?;

        let mut completed = FxHashMap::default();
        let names = pending.keys().cloned().collect::<Vec<_>>();
        for name in names {
            Self::finalize(&name, &graph, &mut pending, &mut completed)?;
        }

        for (name, repository) in completed {
            let entry = self.entries.get_mut(&name).ok_or_else(|| {
                RepoSetError::Internal(anyhow!(
                    "replacement entry for repository '{name}' is missing"
                ))
            })?;
            entry.repository = Some(repository);
        }

        Ok(())
    }

    fn validate_master_graph(graph: &FxHashMap<String, Vec<String>>) -> Result<(), RepoSetError> {
        let mut visiting = FxHashSet::default();
        let mut visited = FxHashSet::default();
        for name in graph.keys() {
            Self::validate_master(name, graph, &mut visiting, &mut visited)?;
        }
        Ok(())
    }

    fn validate_master(
        name: &str,
        graph: &FxHashMap<String, Vec<String>>,
        visiting: &mut FxHashSet<String>,
        visited: &mut FxHashSet<String>,
    ) -> Result<(), RepoSetError> {
        if visited.contains(name) {
            return Ok(());
        }
        let Some(masters) = graph.get(name) else {
            return Ok(());
        };
        if !visiting.insert(name.to_owned()) {
            return Err(RepoSetError::Cycle(name.to_owned()));
        }

        for master in masters {
            Self::validate_master(master, graph, visiting, visited)?;
        }
        visiting.remove(name);
        visited.insert(name.to_owned());
        Ok(())
    }

    fn finalize(
        name: &str,
        graph: &FxHashMap<String, Vec<String>>,
        pending: &mut FxHashMap<String, Repository>,
        completed: &mut FxHashMap<String, Repository>,
    ) -> Result<(), RepoSetError> {
        if completed.contains_key(name) || !pending.contains_key(name) {
            return Ok(());
        }
        let masters = graph.get(name).ok_or_else(|| {
            RepoSetError::Internal(anyhow!("master graph node for '{name}' is missing"))
        })?;

        for master in masters {
            Self::finalize(master, graph, pending, completed)?;
        }

        let mut repository = pending.remove(name).ok_or_else(|| {
            RepoSetError::Internal(anyhow!("pending repository '{name}' is missing"))
        })?;
        for master_name in masters {
            if let Some(master) = completed.get(master_name) {
                repository.inherit_from(master);
            } else {
                debug!("Skipping missing or unavailable master '{master_name}' for '{name}'");
            }
        }

        match repository.populate() {
            Ok(()) => {
                debug!(
                    "Loaded repository '{name}' with {} ebuilds",
                    repository.cpvs().count()
                );
                completed.insert(name.to_owned(), repository);
            }
            Err(
                error @ (RepositoryError::Data(_)
                | RepositoryError::Layout(_)
                | RepositoryError::Profile(_)),
            ) => {
                warn!("Repository '{name}' is unavailable: {error:#}");
            }
            Err(source) => {
                return Err(RepoSetError::repo_failure(name, source));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{RepoBuilder, repo_set};
    use super::*;

    #[test]
    fn test_repository_data_failure() {
        let mut fixture =
            repo_set(vec![RepoBuilder::new("valid"), RepoBuilder::new("invalid")]).unwrap();
        fs::remove_file(
            fixture
                .get("invalid")
                .unwrap()
                .location
                .join("metadata/layout.conf"),
        )
        .unwrap();

        fixture.reload_from_disk().unwrap();

        assert!(fixture.get("valid").is_some());
        assert!(fixture.get("invalid").is_none());
    }

    #[test]
    fn test_sync_makes_repository_available() {
        let temp = tempfile::Builder::new().tempdir().unwrap();
        let location = temp.path().join("repository");
        RepoBuilder::new("repo").write_to(&location).unwrap();
        let config = temp.path().join("repos.conf");
        fs::write(
            &config,
            format!("[repo]\nlocation = {}\n", location.display()),
        )
        .unwrap();
        let mut set = RepoSet::new(&config).unwrap();
        fs::remove_dir_all(&location).unwrap();
        set.reload_from_disk().unwrap();

        RepoBuilder::new("repo")
            .categories(["app-misc"])
            .ebuild("app-misc", "foo", "1", "")
            .write_to(&location)
            .unwrap();
        set.maybe_sync().unwrap();

        assert!(
            set.get("repo").is_some_and(|repository| repository
                .cpvs()
                .any(|cpv| cpv.fqn() == "app-misc/foo-1"))
        );
    }

    #[test]
    fn test_missing_repository() {
        let temp = tempfile::Builder::new().tempdir().unwrap();
        let config = temp.path().join("repos.conf");
        fs::write(
            &config,
            format!(
                "[repo]\nlocation = {}\n",
                temp.path().join("repo").display()
            ),
        )
        .unwrap();

        let error = RepoSet::new(&config).unwrap_err();

        assert!(matches!(error, RepoSetError::Configuration(_)));
    }

    #[test]
    fn test_find_unavailable_repo() {
        let mut fixture = repo_set(vec![
            RepoBuilder::new("repo")
                .repos_conf_property("sync-type", "git")
                .repos_conf_property("sync-uri", "https://example.invalid/repo.git"),
        ])
        .unwrap();
        fs::remove_dir_all(fixture.get("repo").unwrap().location.clone()).unwrap();
        fixture.reload_from_disk().unwrap();

        assert!(
            fixture
                .find_packages(&Atom::new("app-misc/foo::repo").unwrap())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn test_layout_masters_are_used() {
        let fixture = repo_set(vec![
            RepoBuilder::new("master")
                .categories(["app-misc"])
                .eclass("master"),
            RepoBuilder::new("overlay")
                .masters(["master"])
                .categories(["app-misc"])
                .ebuild("app-misc", "foo", "1", ""),
        ])
        .unwrap();

        let overlay = fixture.get("overlay").unwrap();
        let has_package = overlay.cpvs().any(|cpv| cpv.fqn() == "app-misc/foo-1");

        assert!(has_package);
        assert_eq!(overlay.categories.len(), 1);
        assert!(overlay.categories.contains("app-misc"));
        assert!(
            fixture
                .get("overlay")
                .unwrap()
                .eclasses
                .contains_key("master")
        );
    }

    #[test]
    fn test_empty_masters_override() {
        let fixture = repo_set(vec![
            RepoBuilder::new("master")
                .categories(["app-misc"])
                .eclass("master"),
            RepoBuilder::new("overlay")
                .masters(["master"])
                .masters_override()
                .ebuild("app-misc", "foo", "1", ""),
        ])
        .unwrap();

        let has_package = fixture
            .get("overlay")
            .unwrap()
            .cpvs()
            .any(|cpv| cpv.fqn() == "app-misc/foo-1");

        assert!(!has_package);
        assert!(
            !fixture
                .get("overlay")
                .unwrap()
                .eclasses
                .contains_key("master")
        );
    }

    #[test]
    fn test_reload_refreshes_dependent_overlays() {
        let mut fixture = repo_set(vec![
            RepoBuilder::new("master").categories(["app-misc"]),
            RepoBuilder::new("overlay")
                .masters(["master"])
                .ebuild("app-misc", "foo", "1", "")
                .ebuild("dev-libs", "bar", "1", ""),
        ])
        .unwrap();

        let overlay_before = fixture.get("overlay").unwrap();
        assert!(
            !overlay_before
                .cpvs()
                .any(|cpv| cpv.fqn() == "dev-libs/bar-1")
        );
        assert!(!overlay_before.eclasses.contains_key("refreshed"));

        let master_path = fixture.get("master").unwrap().location.as_path();
        fs::write(
            master_path.join("profiles").join("categories"),
            "app-misc\ndev-libs\n",
        )
        .unwrap();
        fs::write(master_path.join("eclass").join("refreshed.eclass"), "").unwrap();
        fixture.reload_from_disk().unwrap();

        let overlay_after = fixture.get("overlay").unwrap();
        assert!(
            overlay_after
                .cpvs()
                .any(|cpv| cpv.fqn() == "dev-libs/bar-1")
        );
        assert!(overlay_after.eclasses.contains_key("refreshed"));
    }

    #[test]
    fn test_master_cycle() {
        let result = repo_set(vec![
            RepoBuilder::new("first").masters(["second"]),
            RepoBuilder::new("second").masters(["first"]),
        ]);

        let Err(error) = result else {
            panic!("master cycle should fail repository-set construction")
        };
        assert!(
            error
                .downcast_ref::<RepoSetError>()
                .is_some_and(|error| matches!(error, RepoSetError::Cycle { .. }))
        );
    }

    #[test]
    fn test_unavailable_master_cycle() {
        let result = repo_set(vec![
            RepoBuilder::new("first").repos_conf_property("masters", "second"),
            RepoBuilder::new("second")
                .formats(["pms"])
                .eapi("0")
                .profile_entries_dir("package.mask", "app-misc/foo\n")
                .repos_conf_property("masters", "first"),
        ]);

        let Err(error) = result else {
            panic!("master cycle should fail reposet construction")
        };
        assert!(
            error
                .downcast_ref::<RepoSetError>()
                .is_some_and(|error| matches!(error, RepoSetError::Cycle { .. }))
        );
    }

    #[test]
    fn test_unavailable_masters() {
        let mut fixture = repo_set(vec![
            RepoBuilder::new("available")
                .categories(["app-misc"])
                .eclass("available"),
            RepoBuilder::new("unavailable")
                .repos_conf_property("sync-type", "git")
                .repos_conf_property("sync-uri", "https://example.invalid/unavailable.git"),
            RepoBuilder::new("child")
                .masters(["missing", "available", "unavailable"])
                .ebuild("app-misc", "foo", "1", ""),
        ])
        .unwrap();

        // Remove the unavailable repo so it simulates a non-existent location
        let unavailable_path = fixture.get("unavailable").unwrap().location.clone();
        fs::remove_dir_all(unavailable_path).unwrap();
        fixture.reload_from_disk().unwrap();

        let child = fixture.get("child").unwrap();

        assert!(fixture.get("unavailable").is_none());
        assert!(child.eclasses.contains_key("available"));
        assert!(child.cpvs().any(|cpv| cpv.fqn() == "app-misc/foo-1"));
    }

    #[test]
    fn test_direct_master_order_is_preserved() {
        let fixture = repo_set(vec![
            RepoBuilder::new("first").eclass("shared"),
            RepoBuilder::new("second").eclass("shared"),
            RepoBuilder::new("child").masters(["first", "second"]),
        ])
        .unwrap();

        let second_path = fixture.get("second").unwrap().location.as_path();
        let shared = fixture
            .get("child")
            .unwrap()
            .eclasses
            .get("shared")
            .unwrap();

        assert!(shared.path.starts_with(second_path));

        let child = fixture.get("child").unwrap();
        let paths = child.eclasses.repo_paths().collect::<Vec<_>>();
        assert_eq!(paths[0], fixture.get("child").unwrap().location.as_path());
        assert_eq!(paths[1], fixture.get("first").unwrap().location.as_path());
        assert_eq!(paths[2], second_path);
    }
}
