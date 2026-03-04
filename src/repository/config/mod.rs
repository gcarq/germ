use crate::regex::REPOSITORY;
use crate::repository::config::layout::Layout;
use crate::utils;
use anyhow::{Context, Result, anyhow};
use ini::Ini;
use log::{debug, warn};
use regex::Regex;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

mod layout;

/// Regex to validate repository names.
static REPO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^{REPOSITORY}$")).unwrap());

// List of properties in repos.conf and layout.conf that are currently not supported.
const UNSUPPORTED_CONF_PROPERTIES: &[&str] = &["aliases", "eclass-overrides", "force"];

pub struct RepoManagerConfig {
    pub main_repo_name: String,
    pub repo_confs: Vec<RepositoryConfig>,
}

impl RepoManagerConfig {
    /// Loads the `repos.conf` file or directory from the given `location` and returns a
    /// [`RepoManagerConfig`] instance.
    ///
    /// If the location is a directory, it loads and merges all files in the directory
    /// except files starting with `.` or ending with `~`.
    pub fn load(location: &Path) -> Result<Self> {
        debug!("Loading repos.conf from '{}' ...", location.display());
        let conf = Self::parse_conf(location).with_context(|| "unable to parse repos.conf")?;

        // For now, use "gentoo" as fallback if no DEFAULT section or main-repo property is defined
        let main_repo_name = conf
            .section(Some("DEFAULT"))
            .and_then(|props| props.get("main-repo").map(|name| name.to_owned()))
            .unwrap_or_else(|| "gentoo".into());

        debug!("Main repository: '{}'", main_repo_name);

        let mut repo_confs = conf
            .into_iter()
            .filter_map(|(section, properties)| match section {
                Some(name) if name != "DEFAULT" => Some((name, properties)),
                _ => None,
            })
            .map(|(name, properties)| {
                RepositoryConfig::new(&name, HashMap::from_iter(properties.into_iter()))
                    .with_context(|| format!("unable to build repository config for '{name}'"))
            })
            .collect::<Result<Vec<_>>>()?;

        // Sort repositories based on their masters to ensure correct resolution order.
        // TODO: This is not a perfect topological sort and may fail for complex master relationships,
        //  but it should work for now.
        repo_confs.sort();

        Ok(Self {
            main_repo_name,
            repo_confs,
        })
    }

    /// Helper function to merge and parse `repos.conf` from the given `location`.
    fn parse_conf(location: &Path) -> Result<Ini> {
        let conf = if location.metadata()?.is_file() {
            fs::read_to_string(location).with_context(|| "failed to load repos.conf")?
        } else {
            utils::list_files(location)?
                .map(|p| match p {
                    Ok(path) => fs::read_to_string(&path)
                        .with_context(|| anyhow!("unable to read file '{}'", path.display())),
                    Err(err) => Err(err),
                })
                .collect::<Result<Vec<_>>>()?
                .join("\n")
        };
        Ini::load_from_str(&conf).with_context(|| "failed to parse repos.conf")
    }
}

/// Represents the configuration of a single repository.
/// Properties taken from `repos.conf` take precedence over `layout.conf` where applicable.
#[derive(Clone, Eq, Debug)]
pub struct RepositoryConfig {
    // The path to the repository on the filesystem
    pub location: PathBuf,
    // Allows overriding `profiles/repo_name`, although discouraged
    pub name: String,
    // Defines parent repositories to resolve ebuilds, eclasses and profiles from
    pub masters: Vec<String>,
    // Defines the repository priority for resolving ebuilds and eclasses
    pub priority: isize,
    // Holds all raw properties from the repository section in repos.conf for potential future use
    pub raw_properties: HashMap<String, String>,
}

impl RepositoryConfig {
    /// Builds a [`RepositoryConfig`] from the given `repo_name` and INI `properties`
    /// from repos.conf.
    ///
    /// Returns `Err` if required properties are missing, if the `layout.conf` file is invalid or if
    /// the repository name doesn't match the name in `layout.conf` or `repo_name` file.
    fn new(repo_name: &str, properties: HashMap<String, String>) -> Result<RepositoryConfig> {
        for prop in UNSUPPORTED_CONF_PROPERTIES {
            if properties.contains_key(*prop) {
                warn!("repos.conf: '{prop}' property is not supported and will be ignored");
            }
        }

        let location = properties
            .get("location")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("missing location property"))?;

        let layout = Layout::from_path(&location.join("metadata").join("layout.conf"))
            .with_context(|| "unable to load layout.conf")?;

        // The name in `layout.conf` takes precedence over `repo_name`
        let name = match layout.name {
            Some(name) => name,
            None => Self::read_repo_name(&location)?,
        };
        // TODO: its unclear which name should take precedence if both layout.conf and repo_name
        //  file exist, see https://bugs.gentoo.org/563874
        if name != repo_name {
            return Err(anyhow!(
                "Repository name mismatch: '{name}' vs '{repo_name}' (from repos.conf)!",
            ));
        }

        let masters = match properties.get("masters") {
            Some(masters) => masters
                .split_ascii_whitespace()
                .map(|s| s.to_owned())
                .collect(),
            None => layout.masters,
        };

        let config = RepositoryConfig {
            location,
            name,
            masters,
            priority: properties
                .get("priority")
                .map(|s| s.parse::<isize>())
                .transpose()
                .with_context(|| "invalid priority value")?
                .unwrap_or(0),
            raw_properties: HashMap::from_iter(
                properties
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string())),
            ),
        };
        Ok(config)
    }

    /// Reads the repository name from `profiles/repo_name`.
    /// The given `location` should be the root of the repository.
    fn read_repo_name(location: &Path) -> Result<String> {
        let repo_name = fs::read_to_string(location.join("profiles").join("repo_name"))?
            .lines()
            .next()
            .ok_or_else(|| anyhow!("Empty repo_name file"))?
            .to_owned();
        if !REPO_RE.is_match(&repo_name) {
            return Err(anyhow!(
                "Invalid repository name: {repo_name}. It must match the regex: {}",
                REPO_RE.as_str()
            ));
        }
        Ok(repo_name)
    }
}

impl Ord for RepositoryConfig {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.masters.contains(&other.name) {
            return Ordering::Greater;
        }
        if other.masters.contains(&self.name) {
            return Ordering::Less;
        }
        self.masters.len().cmp(&other.masters.len())
    }
}

impl PartialEq<Self> for RepositoryConfig {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl PartialOrd<Self> for RepositoryConfig {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_conf_order() {
        let local = RepositoryConfig {
            name: "testing".into(),
            location: PathBuf::from("/dev/null"),
            masters: vec!["gentoo".into(), "guru".into()],
            priority: 0,
            raw_properties: HashMap::new(),
        };
        let testing = RepositoryConfig {
            name: "testing".into(),
            location: PathBuf::from("/dev/null"),
            masters: vec!["gentoo".into(), "kde".into()],
            priority: 0,
            raw_properties: HashMap::new(),
        };
        let gentoo = RepositoryConfig {
            name: "gentoo".into(),
            location: PathBuf::from("/dev/null"),
            masters: vec![],
            priority: 0,
            raw_properties: HashMap::new(),
        };
        let guru = RepositoryConfig {
            name: "guru".into(),
            location: PathBuf::from("/dev/null"),
            masters: vec!["gentoo".into()],
            priority: 0,
            raw_properties: HashMap::new(),
        };
        let kde = RepositoryConfig {
            name: "kde".into(),
            location: PathBuf::from("/dev/null"),
            masters: vec!["gentoo".into()],
            priority: 0,
            raw_properties: HashMap::new(),
        };

        let mut configs = vec![
            local.clone(),
            testing.clone(),
            gentoo.clone(),
            guru.clone(),
            kde.clone(),
        ];
        configs.sort();
        assert_eq!(configs, vec![gentoo, guru, kde, local, testing]);
    }

    #[test]
    fn test_repository_regex_match() {
        let valid_names = ["gentoo", "my-repo_1", "repo123"];
        for name in valid_names {
            assert!(
                REPO_RE.is_match(name),
                "repository name '{name}' should be valid",
            );
        }
    }

    #[test]
    fn test_repository_regex_no_match() {
        let invalid_names = ["", "my repo", "repo!", "repo@123", "repo#name", "-repo"];
        for name in invalid_names {
            assert!(
                !REPO_RE.is_match(name),
                "repository name '{name}' should be invalid",
            );
        }
    }
}
