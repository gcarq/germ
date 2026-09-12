mod config;
mod error;
mod sync;

use self::config::RepoSetConfig;
pub use self::error::RepoSetError;
use self::sync::{SyncHandler, build_sync_handler};
use super::RepoName;
use super::tree::{Repository, RepositoryError};
use crate::SysConf;
use crate::deps::atom::Atom;
use crate::policy::pkgmask::RepositorySource;
use crate::profile::Profile;
use crate::repository::Arch;
use crate::repository::tree::PackageResult;
use crate::types::FxHashMap;
use crate::utils::{DfsState, Inherit, Visit};
use anyhow::anyhow;
use either::Either;
use indexmap::IndexMap;
use log::{debug, error, warn};
use std::path::Path;
use std::sync::Arc;
use std::{fs, io};

/// Resolves and handles all available [`Repository`] instances.
///
/// It gets configured via `repos.conf` that usually resides in `/etc/portage/`.
/// See <https://dev.gentoo.org/~zmedico/portage/doc/man/portage.5.html>
#[cfg_attr(test, derive(Default, Debug))]
pub struct RepoSet {
    sysconf: Arc<SysConf>,
    config: RepoSetConfig,
    entries: IndexMap<RepoName, RepositoryEntry>,
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
    fn sync(&self, name: &RepoName, force: bool) -> anyhow::Result<()> {
        match &self.sync_handler {
            Some(handler) => handler.sync(name, force),
            None if force => Err(anyhow!("repository has no sync handler")),
            None => Ok(()),
        }
    }
}

impl RepoSet {
    /// Builds a [`RepoSet`] with the given runtime system configuration.
    pub fn new(sysconf: Arc<SysConf>) -> Result<Self, RepoSetError> {
        let repos_conf = sysconf.portage_conf().join("repos.conf");
        let config = RepoSetConfig::load(&repos_conf).map_err(|err| {
            RepoSetError::Configuration(err.context(format!(
                "unable to load repository configuration from {}",
                repos_conf.display()
            )))
        })?;

        let mut set = Self {
            config,
            sysconf,
            entries: IndexMap::default(),
        };
        set.reload_from_disk()?;
        Ok(set)
    }

    /// Eagerly resolves and returns all packages that match the given `atom`.
    /// TODO: Order the returned packages by version
    pub async fn find_packages(&mut self, atom: &Atom) -> Result<Vec<PackageResult>, RepoSetError> {
        let mut results = Vec::new();

        let repos = match atom.repo() {
            Some(repo) => Either::Left(self.get_mut(repo).into_iter()),
            None => Either::Right(self.iter_mut()),
        };

        for repo in repos {
            let name = repo.name().clone();
            results.extend(
                repo.find_packages(atom)
                    .await
                    .map_err(|err| RepoSetError::repo_failure(&name, err))?,
            );
        }
        Ok(results)
    }

    /// Attempts to synchronize all repos and reloads repo data from disk.
    ///
    /// If `repo` is provided, only it will be synced, overriding `auto-sync`,
    /// otherwise all repos with `auto-sync` enabled will be synced.
    /// A sync failure is logged as error but doesn't return an `Err`.
    pub fn maybe_sync(&mut self, repo: Option<&str>) -> Result<(), RepoSetError> {
        if let Some(name) = repo {
            let name = RepoName::new(name).map_err(RepoSetError::Sync)?;
            let entry = self
                .entries
                .get(&name)
                .ok_or_else(|| RepoSetError::Sync(anyhow!("unknown repository '{name}'")))?;
            entry.sync(&name, true).map_err(|err| {
                RepoSetError::Sync(err.context(anyhow!("unable to sync repository '{name}'")))
            })?;
        } else {
            for (name, entry) in &self.entries {
                if let Err(err) = entry.sync(name, false) {
                    error!("Failed to sync repository '{name}': {err}");
                }
            }
        }

        self.reload_from_disk()
    }

    /// Returns a repository reference for the given `name`.
    pub fn get(&self, name: impl AsRef<str>) -> Option<&Repository> {
        self.entries.get(name.as_ref())?.repository.as_ref()
    }

    /// Returns a mutable repository reference for the given `name`.
    pub fn get_mut(&mut self, name: impl AsRef<str>) -> Option<&mut Repository> {
        self.entries.get_mut(name.as_ref())?.repository.as_mut()
    }

    /// Returns an iterator over all available repos ordered by priority.
    pub fn iter(&self) -> impl Iterator<Item = &Repository> {
        self.entries
            .values()
            .filter_map(|entry| entry.repository.as_ref())
    }

    /// Returns a mutable iterator over all available repos ordered by priority.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Repository> {
        self.entries
            .values_mut()
            .filter_map(|entry| entry.repository.as_mut())
    }

    /// Returns an iterator that drains all available repos ordered by priority.
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

    /// Resolves a profile using the available repos.
    pub fn resolve_profile(&self, location: &Path) -> anyhow::Result<Profile> {
        Profile::resolve(location, self)
    }

    /// Validates that at least one available repository supports the given architecture.
    pub fn validate_arch(&self, arch: &Arch) -> anyhow::Result<()> {
        if !self.iter().any(|repo| repo.supports_arch(arch)) {
            anyhow::bail!("ARCH '{arch}' is not supported by any configured repository");
        }
        Ok(())
    }

    /// Validates that a profile is described by at least one available repository for `arch`.
    pub fn validate_profile(&self, profile: &Profile, arch: &Arch) -> anyhow::Result<()> {
        for repo in self.iter() {
            let profile_prefix = format!("{}/profiles/", repo.location().display());
            if let Ok(path) = profile.location.strip_prefix(&profile_prefix)
                && repo.is_known_profile(arch, path)
            {
                return Ok(());
            }
        }
        anyhow::bail!("Profile {profile} is not valid for any configured repository")
    }

    /// Returns the aggregated package mask/unmask entries from all available repos.
    pub fn package_mask_source(&self) -> anyhow::Result<RepositorySource> {
        let mut source = RepositorySource::default();
        for repo in self.iter() {
            source.mask.inherit_from(&repo.package_mask)?;
            source.unmask.inherit_from(&repo.package_unmask)?;
        }
        Ok(source)
    }

    /// Reloads all repository data from disk.
    fn reload_from_disk(&mut self) -> Result<(), RepoSetError> {
        let mut entries = IndexMap::default();
        for config in self.config.iter() {
            let sync_handler = build_sync_handler(&config.raw_properties).map_err(|error| {
                RepoSetError::Configuration(
                    error.context(format!("unable to configure repository '{}'", config.name)),
                )
            })?;

            // IndexMap preserves the order of insertion, which is important for repository priority.
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
            let sysconf = self.sysconf.clone();
            match fs::metadata(&config.location) {
                Ok(_) => match Repository::load(&config.name, &config.location, sysconf) {
                    Ok(repo) => {
                        pending.insert(config.name.clone(), repo);
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
                        .map(|repo| repo.layout().masters.clone())
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

        for (name, repo) in completed {
            let entry = self.entries.get_mut(&name).ok_or_else(|| {
                RepoSetError::Internal(anyhow!(
                    "replacement entry for repository '{name}' is missing"
                ))
            })?;
            entry.repository = Some(repo);
        }

        Ok(())
    }

    /// Validates that the given `graph` is not cyclic.
    fn validate_master_graph(
        graph: &FxHashMap<RepoName, Vec<RepoName>>,
    ) -> Result<(), RepoSetError> {
        fn visit(
            name: &RepoName,
            graph: &FxHashMap<RepoName, Vec<RepoName>>,
            dfs: &mut DfsState<RepoName>,
        ) -> Result<(), RepoSetError> {
            match dfs.enter(name) {
                Ok(Visit::AlreadyVisited) => return Ok(()),
                Ok(Visit::Continue) => {}
                Err(_) => return Err(RepoSetError::Cycle(name.to_string())),
            }

            if let Some(masters) = graph.get(name) {
                for master in masters {
                    visit(master, graph, dfs)?;
                }
            }

            dfs.leave(name);
            Ok(())
        }

        let mut dfs = DfsState::default();
        for name in graph.keys() {
            visit(name, graph, &mut dfs)?;
        }
        Ok(())
    }

    fn finalize(
        name: &RepoName,
        graph: &FxHashMap<RepoName, Vec<RepoName>>,
        pending: &mut FxHashMap<RepoName, Repository>,
        completed: &mut FxHashMap<RepoName, Repository>,
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

        let mut repo = pending.remove(name).ok_or_else(|| {
            RepoSetError::Internal(anyhow!("pending repository '{name}' is missing"))
        })?;
        for master_name in masters {
            if let Some(master) = completed.get(master_name) {
                repo.inherit_from(master).map_err(|source| {
                    RepoSetError::repo_failure(name, RepositoryError::Internal(source))
                })?;
            } else {
                debug!("Skipping missing or unavailable master '{master_name}' for '{name}'");
            }
        }

        match repo.finalize() {
            Ok(()) => {
                debug!("Loaded repository '{name}'");
                completed.insert(name.to_owned(), repo);
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
    use crate::files::entry::Precedence;

    #[test]
    fn test_validate_arch() -> anyhow::Result<()> {
        let reposet = repo_set([RepoBuilder::new("repo")])?;

        assert!(reposet.validate_arch(&"amd64".parse()?).is_ok());
        assert!(reposet.validate_arch(&"arm64".parse()?).is_err());
        Ok(())
    }

    #[test]
    fn test_validate_profile() -> anyhow::Result<()> {
        let reposet = repo_set([RepoBuilder::new("repo")
            .profile("default/linux")
            .profile("other")])?;
        let repository = reposet.get("repo").unwrap();
        let valid = Profile::resolve(
            &repository.location().join("profiles/default/linux"),
            &reposet,
        )?;
        let invalid = Profile::resolve(&repository.location().join("profiles/other"), &reposet)?;

        assert!(reposet.validate_profile(&valid, &"amd64".parse()?).is_ok());
        assert!(
            reposet
                .validate_profile(&invalid, &"amd64".parse()?)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn test_package_mask_source() -> anyhow::Result<()> {
        let reposet = repo_set([RepoBuilder::new("repo")
            .profile_file("package.mask", "dev-lang/rust")
            .profile_file("package.unmask", "app-editors/vim")])?;

        let source = reposet.package_mask_source()?;
        let mask = source.mask.into_iter().next().expect("repository mask");
        let unmask = source.unmask.into_iter().next().expect("repository unmask");

        assert_eq!(mask.to_string(), "dev-lang/rust");
        assert_eq!(mask.prec, Precedence::Repository);
        assert_eq!(unmask.to_string(), "app-editors/vim");
        assert_eq!(unmask.prec, Precedence::Repository);
        Ok(())
    }

    #[test]
    fn test_repository_data_failure() {
        let mut reposet =
            repo_set(vec![RepoBuilder::new("valid"), RepoBuilder::new("invalid")]).unwrap();
        fs::remove_file(
            reposet
                .get("invalid")
                .unwrap()
                .location()
                .join("metadata/layout.conf"),
        )
        .unwrap();

        reposet.reload_from_disk().unwrap();

        assert!(reposet.get("valid").is_some());
        assert!(reposet.get("invalid").is_none());
    }

    #[test]
    fn test_sync_reloads_repository() {
        let temp = tempfile::Builder::new().tempdir().unwrap();
        let location = temp.path().join("repository");
        RepoBuilder::new("repo").write_to(&location).unwrap();
        let sysconf = SysConf::new(temp.path().to_path_buf());
        fs::create_dir_all(sysconf.portage_conf()).unwrap();
        let config = sysconf.portage_conf().join("repos.conf");
        fs::write(
            &config,
            format!("[repo]\nlocation = {}\n", location.display()),
        )
        .unwrap();
        let mut set = RepoSet::new(sysconf.into()).unwrap();
        fs::remove_dir_all(&location).unwrap();
        set.reload_from_disk().unwrap();

        RepoBuilder::new("repo")
            .categories(["app-misc"])
            .ebuild("app-misc", "foo", "1", "")
            .write_to(&location)
            .unwrap();
        set.maybe_sync(None).unwrap();

        assert!(
            set.get_mut("repo")
                .is_some_and(|repo| repo.cpvs().any(|cpv| cpv.fqn() == "app-misc/foo-1"))
        );
    }

    #[test]
    fn test_missing_repository() {
        let temp = tempfile::Builder::new().tempdir().unwrap();
        let sysconf = SysConf::new(temp.path().to_path_buf());
        fs::create_dir_all(sysconf.portage_conf()).unwrap();
        let config = sysconf.portage_conf().join("repos.conf");
        fs::write(
            &config,
            format!(
                "[repo]\nlocation = {}\n",
                temp.path().join("repo").display()
            ),
        )
        .unwrap();

        let error = RepoSet::new(sysconf.into()).unwrap_err();

        assert!(matches!(error, RepoSetError::Configuration(_)));
    }

    #[tokio::test]
    async fn test_find_unavailable_repo() {
        let mut reposet = repo_set(vec![
            RepoBuilder::new("repo")
                .repos_conf_property("sync-type", "git")
                .repos_conf_property("sync-uri", "https://example.invalid/repo.git"),
        ])
        .unwrap();
        fs::remove_dir_all(reposet.get("repo").unwrap().location()).unwrap();
        reposet.reload_from_disk().unwrap();

        assert!(
            reposet
                .find_packages(&Atom::new("app-misc/foo::repo").unwrap())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn test_layout_masters_are_used() {
        let mut fixture = repo_set(vec![
            RepoBuilder::new("master")
                .categories(["app-misc"])
                .eclass("master"),
            RepoBuilder::new("overlay")
                .masters(["master"])
                .ebuild("app-misc", "foo", "1", ""),
        ])
        .unwrap();

        let overlay = fixture.get_mut("overlay").unwrap();
        let has_package = overlay.cpvs().any(|cpv| cpv.fqn() == "app-misc/foo-1");

        assert!(has_package);
        assert!(
            fixture
                .get("overlay")
                .unwrap()
                .eclasses()
                .contains_key("master")
        );
    }

    #[test]
    fn test_empty_masters_override() {
        let mut reposet = repo_set(vec![
            RepoBuilder::new("master")
                .categories(["app-misc"])
                .eclass("master"),
            RepoBuilder::new("overlay")
                .masters(["master"])
                .masters_override()
                .ebuild("app-misc", "foo", "1", ""),
        ])
        .unwrap();

        let has_package = reposet
            .get_mut("overlay")
            .unwrap()
            .cpvs()
            .any(|cpv| cpv.fqn() == "app-misc/foo-1");

        assert!(!has_package);
        assert!(
            !reposet
                .get("overlay")
                .unwrap()
                .eclasses()
                .contains_key("master")
        );
    }

    #[test]
    fn test_reload_refreshes_dependent_overlays() {
        let mut reposet = repo_set(vec![
            RepoBuilder::new("master").categories(["app-misc"]),
            RepoBuilder::new("overlay")
                .masters(["master"])
                .ebuild("app-misc", "foo", "1", "")
                .ebuild("dev-libs", "bar", "1", ""),
        ])
        .unwrap();

        let overlay_before = reposet.get_mut("overlay").unwrap();
        assert!(
            !overlay_before
                .cpvs()
                .any(|cpv| cpv.fqn() == "dev-libs/bar-1")
        );
        assert!(!overlay_before.eclasses().contains_key("refreshed"));

        let master_path = reposet.get("master").unwrap().location();
        fs::write(
            master_path.join("profiles").join("categories"),
            "app-misc\ndev-libs\n",
        )
        .unwrap();
        fs::write(master_path.join("eclass").join("refreshed.eclass"), "").unwrap();
        reposet.reload_from_disk().unwrap();

        let overlay_after = reposet.get_mut("overlay").unwrap();
        assert!(
            overlay_after
                .cpvs()
                .any(|cpv| cpv.fqn() == "dev-libs/bar-1")
        );
        assert!(overlay_after.eclasses().contains_key("refreshed"));
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
        let mut reposet = repo_set(vec![
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
        let unavailable_path = reposet.get("unavailable").unwrap().location();
        fs::remove_dir_all(unavailable_path).unwrap();
        reposet.reload_from_disk().unwrap();
        assert!(reposet.get_mut("unavailable").is_none());

        let child = reposet.get_mut("child").unwrap();
        assert!(child.eclasses().contains_key("available"));
        assert!(child.cpvs().any(|cpv| cpv.fqn() == "app-misc/foo-1"));
    }

    #[test]
    fn test_direct_master_order_is_preserved() {
        let reposet = repo_set(vec![
            RepoBuilder::new("first").eclass("shared"),
            RepoBuilder::new("second").eclass("shared"),
            RepoBuilder::new("child").masters(["first", "second"]),
        ])
        .unwrap();

        let second_path = reposet.get("second").unwrap().location();
        let shared = reposet
            .get("child")
            .unwrap()
            .eclasses()
            .get("shared")
            .unwrap();

        assert!(shared.path.starts_with(second_path));

        let child = reposet.get("child").unwrap();
        let paths = child.eclasses().repo_paths().collect::<Vec<_>>();
        assert_eq!(paths[0], reposet.get("child").unwrap().location());
        assert_eq!(paths[1], reposet.get("first").unwrap().location());
        assert_eq!(paths[2], second_path);
    }

    #[test]
    fn test_priority_order() -> anyhow::Result<()> {
        let reposet = repo_set([
            RepoBuilder::new("fallback").repos_conf_property("priority", "10"),
            RepoBuilder::new("preferred").repos_conf_property("priority", "-10"),
        ])?;

        let names = reposet
            .iter()
            .map(|repo| repo.name().as_str())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["preferred", "fallback"]);
        Ok(())
    }
}
