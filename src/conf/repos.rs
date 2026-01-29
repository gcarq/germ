use crate::repository::Repository;
use crate::utils;
use anyhow::{Context, Result, anyhow};
use ini::Ini;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::{fmt, fs};

/// Holds all repository configuration that usually resides in /etc/portage/repos.conf.
/// TODO implement missing functionality listed below.
///  See https://dev.gentoo.org/~zmedico/portage/doc/man/portage.5.html
///   * support PORTAGE_REPOSITORIES environment variable
///   * support 'masters' property in repository sections
///   * support 'eclass-overrides' properties in DEFAULT and repository sections
///   * support 'priority' property in repository sections
///   * support 'aliases' property in repository sections
///
///
///
///
#[derive(Debug)]
pub struct ReposConf {
    main_repo_name: String,
    repos: HashMap<String, Repository>,
}

impl ReposConf {
    pub fn new(path: &Path) -> Result<Self> {
        let conf = Self::load_conf(path).with_context(|| "unable to load repos.conf")?;
        let conf = Ini::load_from_str(&conf).with_context(|| "failed to parse repos.conf")?;
        let main_repo =
            Self::resolve_main_repo(&conf).with_context(|| "unable to resolve main repo")?;
        let mut repos = Self::collect_overlays(&conf, &main_repo)
            .with_context(|| "unable to collect overlays")?;
        let main_repo_name = main_repo.name.clone();
        repos.insert(main_repo_name.clone(), main_repo);
        Ok(Self {
            main_repo_name,
            repos,
        })
    }

    /// Returns the repository with the given name, if it exists.
    pub fn get(&self, name: &str) -> Option<&Repository> {
        self.repos.get(name)
    }

    /// Returns the main repository.
    pub fn main_repo(&self) -> &Repository {
        self.get(&self.main_repo_name)
            .expect("FATAL: no main repository assigned")
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Repository)> {
        self.repos.iter()
    }

    pub fn repositories(&self) -> Vec<&Repository> {
        self.repos.values().collect()
    }

    /// Loads the repos.conf configuration from the given path.
    /// If the path is a directory, it loads and merges all files in the directory
    /// except files starting with '.' or ending with '~'.
    fn load_conf(path: &Path) -> Result<String> {
        if path.metadata()?.is_file() {
            return fs::read_to_string(path).with_context(|| "Failed to load repos.conf");
        }

        let merged_conf = utils::files_from_dir(path)?
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

        conf.section(Some(name))
            .ok_or_else(|| anyhow!("missing the section for '{name}'"))?
            .get("location")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("missing location property for repo '{name}'"))
            .and_then(Repository::build_main_repo_from_path)
    }

    /// Collects all overlay repositories from the given repo `conf`,
    /// excluding the main repository.
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
                    let path = properties
                        .get("location")
                        .map(PathBuf::from)
                        .ok_or_else(|| anyhow!("missing location property for repo '{}'", name))?;
                    let repo = Repository::build_overlay_from_path(path, main_repo)
                        .with_context(|| anyhow!("failed to build overlay '{name}'"))?;
                    repos.insert(repo.name.clone(), repo);
                }
                _ => {}
            }
        }
        Ok(repos)
    }
}

impl fmt::Display for ReposConf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (name, repo) in &self.repos {
            writeln!(f, "{}\n    location: {}\n", name, repo.path.display())?;
        }
        Ok(())
    }
}
