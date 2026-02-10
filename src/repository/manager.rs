use crate::ebuild::Ebuild;
use crate::package::Package;
use crate::repository::Repository;
use crate::utils;
use anyhow::{Context, Result, anyhow};
use ini::Ini;
use std::collections::HashMap;
use std::path::Path;
use std::{fmt, fs, iter};

/// Resolves and handles all available [`Repository`] instances.
///
/// It gets configured via `repos.conf` that usually resides in `/etc/portage/`.
/// See <https://dev.gentoo.org/~zmedico/portage/doc/man/portage.5.html>
///
/// TODO implement missing functionality listed below.
///   * support `PORTAGE_REPOSITORIES` environment variable
///   * support `masters` property in repository sections
///   * support `eclass-overrides` properties in DEFAULT and repository sections
///   * support `priority` property in repository sections
///   * support `aliases` property in repository sections
///
pub struct RepoManager {
    main_repo_name: String,
    repositories: HashMap<String, Repository>,
}

impl RepoManager {
    /// Builds a [`RepoManager`] from the repos.conf configuration from the given `location`.
    pub fn new(location: &Path) -> Result<Self> {
        let conf = Self::load_conf(location).with_context(|| "unable to load repos.conf")?;
        let conf = Ini::load_from_str(&conf).with_context(|| "failed to parse repos.conf")?;
        let main_repo =
            Self::resolve_main_repo(&conf).with_context(|| "unable to resolve main repo")?;
        let mut repositories = Self::collect_overlays(&conf, &main_repo)
            .with_context(|| "unable to collect overlays")?;
        let main_repo_name = main_repo.name.clone();
        repositories.insert(main_repo_name.clone(), main_repo);
        Ok(Self {
            main_repo_name,
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

    /// Loads the repos.conf configuration from the given `location`.
    /// If the location is a directory, it loads and merges all files in the directory
    /// except files starting with `.` or ending with `~`.
    fn load_conf(location: &Path) -> Result<String> {
        if location.metadata()?.is_file() {
            return fs::read_to_string(location).with_context(|| "Failed to load repos.conf");
        }

        let merged_conf = utils::files_from_dir(location)?
            .map(|p| match p {
                Ok(path) => fs::read_to_string(&path)
                    .with_context(|| anyhow!("unable to read file {}", path.display())),
                Err(err) => Err(err),
            })
            .collect::<Result<Vec<_>>>()?
            .join("\n");
        Ok(merged_conf)
    }

    /// Resolves the main repository from the given repo `conf`
    /// and returns the repository name found in the configuration and the repository instance.
    fn resolve_main_repo(conf: &Ini) -> Result<Repository> {
        let name = conf
            .section(Some("DEFAULT"))
            .ok_or_else(|| anyhow!("missing DEFAULT section"))?
            .get("main-repo")
            .ok_or_else(|| anyhow!("DEFAULT section missing main-repo property"))?;

        let properties = conf
            .section(Some(name))
            .ok_or_else(|| anyhow!("missing the section for '{name}'"))?;
        Repository::build_main_repository(properties)
            .with_context(|| anyhow!("failed to build main repo '{name}'"))
    }

    /// Collects all overlay repositories from the given repo `conf`,
    /// excluding the main repository specified by `main_repo`.
    fn collect_overlays(conf: &Ini, main_repo: &Repository) -> Result<HashMap<String, Repository>> {
        let mut repos = HashMap::new();
        for (section, properties) in conf.iter() {
            match section {
                Some(name) if name != main_repo.name && name != "DEFAULT" => {
                    if properties.get("masters").is_some() {
                        return Err(anyhow!(
                            "TODO: Repository {name} specifies masters, which is not supported yet"
                        ));
                    }
                    let repo = Repository::build_overlay(properties, main_repo)
                        .with_context(|| anyhow!("failed to build overlay '{name}'"))?;
                    repos.insert(repo.name.clone(), repo);
                }
                _ => {}
            }
        }
        Ok(repos)
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
            writeln!(f, "{}\n    location: {}\n", name, repo.location.display())?;
        }
        Ok(())
    }
}
