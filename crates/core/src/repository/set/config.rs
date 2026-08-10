use crate::regex::REPO_RE;
use crate::types::FxHashMap;
use crate::utils;
use anyhow::{Context, anyhow, bail};
use ini::Ini;
use log::{debug, warn};
use std::fs;
use std::path::{Path, PathBuf};

// List of properties in repos.conf that are currently not supported.
const UNSUPPORTED_CONF_PROPERTIES: &[&str] = &["aliases", "auto-sync", "eclass-overrides", "force"];

#[cfg_attr(test, derive(Default, Debug))]
pub struct RepoSetConfig {
    repo_confs: Vec<RepositoryConfig>,
}

impl RepoSetConfig {
    /// Loads the `repos.conf` file or directory from the given `location` and returns a
    /// [`RepoSetConfig`] instance.
    ///
    /// If the location is a directory, it loads and merges all files in the directory
    /// except files starting with `.` or ending with `~`.
    pub fn load(location: &Path) -> anyhow::Result<Self> {
        debug!("Loading repos.conf from '{}' ...", location.display());
        let conf = Self::parse_conf(location).with_context(|| "unable to parse repos.conf")?;

        let repo_confs = conf
            .into_iter()
            .filter_map(|(section, properties)| match section {
                Some(name) if name != "DEFAULT" => Some((name, properties)),
                _ => None,
            })
            .map(|(name, properties)| {
                RepositoryConfig::new(&name, properties.into_iter().collect::<FxHashMap<_, _>>())
                    .with_context(|| format!("unable to build repository config for '{name}'"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok(Self { repo_confs })
    }

    pub fn iter(&self) -> impl Iterator<Item = &RepositoryConfig> {
        self.repo_confs.iter()
    }

    /// Helper function to merge and parse `repos.conf` from the given `location`.
    fn parse_conf(location: &Path) -> anyhow::Result<Ini> {
        let conf = if location.metadata()?.is_file() {
            fs::read_to_string(location).with_context(|| "failed to load repos.conf")?
        } else {
            utils::list_files(location)
                .map(|p| match p {
                    Ok(path) => fs::read_to_string(&path)
                        .with_context(|| anyhow!("unable to read file '{}'", path.display())),
                    Err(err) => Err(err),
                })
                .collect::<anyhow::Result<Vec<_>>>()?
                .join("\n")
        };
        Ini::load_from_str(&conf).with_context(|| "failed to parse repos.conf")
    }
}

/// Represents the configuration of a single repository from `repos.conf`.
#[derive(Clone)]
#[cfg_attr(test, derive(Default, Debug))]
pub struct RepositoryConfig {
    // The path to the repository on the filesystem
    pub location: PathBuf,
    // The configured canonical repository name from the repos.conf section name
    pub name: String,
    // Defines parent repositories from repos.conf, if explicitly configured
    pub masters: Option<Vec<String>>,
    // Holds all raw properties from the repository section in repos.conf for potential future use
    pub raw_properties: FxHashMap<String, String>,
}

impl RepositoryConfig {
    /// Builds a [`RepositoryConfig`] from the given `repo_name` and INI `properties`
    /// from repos.conf.
    fn new(
        repo_name: &str,
        properties: FxHashMap<String, String>,
    ) -> anyhow::Result<RepositoryConfig> {
        if !REPO_RE.is_match(repo_name) {
            bail!("Invalid repository name: {repo_name}");
        }

        for prop in UNSUPPORTED_CONF_PROPERTIES {
            if properties.contains_key(*prop) {
                warn!("repos.conf: '{prop}' property is not supported and will be ignored");
            }
        }

        let location = properties
            .get("location")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("missing location property"))?;

        let has_sync_defined =
            properties.contains_key("sync-type") && properties.contains_key("sync-uri");

        if !location.exists() && !has_sync_defined {
            bail!(
                "Repository '{repo_name}' has no complete sync configuration and location '{}' is inaccessible",
                location.display()
            );
        }

        let mut raw_properties = properties.clone();
        raw_properties.insert(
            "location".into(),
            location
                .to_str()
                .ok_or_else(|| anyhow!("invalid UTF-8 in path '{}'", location.display()))?
                .to_owned(),
        );

        let masters = properties.get("masters").map(|masters| {
            masters
                .split_ascii_whitespace()
                .map(ToOwned::to_owned)
                .collect()
        });

        let config = RepositoryConfig {
            location,
            name: repo_name.to_owned(),
            masters,
            raw_properties,
        };
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_incomplete_sync_config() {
        let temp = tempfile::Builder::new().tempdir().unwrap();
        let location = temp.path().join("missing");
        let path = temp.path().join("repos.conf");
        fs::write(
            &path,
            format!(
                "[gentoo]\nlocation = {}\nsync-uri = https://example.invalid/gentoo.git\n",
                location.display()
            ),
        )
        .unwrap();

        assert!(RepoSetConfig::load(&path).is_err());
    }

    #[test]
    fn test_invalid_repo_name() {
        let temp = tempfile::Builder::new().tempdir().unwrap();
        let location = temp.path().join("missing");
        let path = temp.path().join("repos.conf");
        fs::write(
            &path,
            format!(
                "[invalid name]\nlocation = {}\nsync-type = git\nsync-uri = https://example.invalid/repo.git\n",
                location.display()
            ),
        ).unwrap();

        assert!(RepoSetConfig::load(&path).is_err());
    }

    #[test]
    fn test_merge_repo_confs() -> anyhow::Result<()> {
        let temp = tempfile::Builder::new().tempdir()?;
        let repos_conf = temp.path().join("repos.conf");
        fs::create_dir(&repos_conf)?;
        fs::write(
            repos_conf.join("gentoo.conf"),
            r#"[gentoo]
                sync-type = git
                sync-uri = https://github.com/gentoo-mirror/gentoo.git
                location = gentoo
                "#,
        )?;
        fs::write(
            repos_conf.join("guru.conf"),
            r#"[guru]
                sync-type = git
                sync-uri = https://github.com/gentoo/guru.git
                location = guru
                "#,
        )?;
        let config = RepoSetConfig::load(&repos_conf)?;

        assert_eq!(config.repo_confs.len(), 2);
        assert_eq!(config.repo_confs[0].name, "gentoo");
        assert_eq!(config.repo_confs[1].name, "guru");
        Ok(())
    }
}
