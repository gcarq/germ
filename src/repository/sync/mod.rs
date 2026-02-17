//! This module defines the synchronization mechanism for repositories.
//! Currently only `git` is supported.

mod git;

use crate::repository::sync::git::GitSyncHandler;
use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::path::PathBuf;

enum SyncType {
    Git,
}

impl SyncType {
    pub fn new(sync_type: &str) -> Result<Self> {
        match sync_type {
            "git" => Ok(SyncType::Git),
            _ => Err(anyhow!("unsupported sync-type: '{sync_type}'")),
        }
    }
}

/// Configuration for the synchronization mechanism.
/// This struct holds common options that are used for all [`SyncType`],
/// such as `location`, `auto_sync`, `sync_uri`, etc.
struct SyncConfig {
    // Absolute path to the repository location on the local filesystem.
    pub location: PathBuf,
    pub auto_sync: bool,
    pub sync_uri: Option<String>,
}

impl SyncConfig {
    pub fn from_ini(properties: &HashMap<String, String>) -> Result<Self> {
        let location = properties
            .get("location")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("missing required 'location' property"))?
            .canonicalize()?;

        let auto_sync = properties
            .get("auto-sync")
            .map(|s| match s.as_str() {
                "true" | "yes" => Ok(true),
                "false" | "no" => Ok(false),
                _ => Err(anyhow!("invalid auto-sync value: '{s}'")),
            })
            .transpose()
            .with_context(|| "invalid auto-sync value")?
            .unwrap_or(true);

        let sync_uri = properties.get("sync-uri").map(|s| s.to_owned());

        Ok(SyncConfig {
            location,
            auto_sync,
            sync_uri,
        })
    }
}

/// Builds a synchronization handler based on the provided INI `properties`.
/// The `sync-type` property is used to determine which synchronization mechanism to use.
pub fn build_sync_handler(
    properties: &HashMap<String, String>,
) -> Result<Option<Box<dyn SyncHandler>>> {
    let sync_type = properties
        .get("sync-type")
        .map(|sync_type| SyncType::new(sync_type))
        .transpose()
        .with_context(|| "invalid sync-type value")?;

    let handler = match sync_type {
        Some(SyncType::Git) => GitSyncHandler::new(properties)?,
        None => return Ok(None),
    };
    Ok(Some(Box::new(handler)))
}

/// Trait for handling repository synchronization.
/// This trait can be implemented for different synchronization mechanisms such as git, rsync, etc.
pub trait SyncHandler {
    /// Creates a new instance of the `SyncHandler` based on the provided INI `properties`
    /// for this repository coming from `repos.conf`.
    fn new(properties: &HashMap<String, String>) -> Result<Self>
    where
        Self: Sized;

    /// Conditionally syncs the repository using either `init` or `update`.
    fn sync(&self) -> Result<()> {
        match self.is_initialized() {
            true => self.update(),
            false => self.init(),
        }
    }

    /// Checks if the repository is already initialized.
    /// This should be used to determine whether to call `init` or `update` in the `sync` method.
    fn is_initialized(&self) -> bool;

    /// Initializes and downloads the repository.
    fn init(&self) -> Result<()>;

    /// Updates an existing repository.
    fn update(&self) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ini::Ini;

    fn load_properties(ini_content: &str) -> HashMap<String, String> {
        Ini::load_from_str(ini_content)
            .expect("failed to parse INI content")
            .section(Some("gentoo"))
            .expect("missing section")
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    }

    #[test]
    fn test_build_sync_handler_git() {
        let ini_content = r#"
                [gentoo]
                location = /tmp
                sync-type = git
            "#;
        let properties = load_properties(ini_content);

        let handler = build_sync_handler(&properties)
            .expect("failed to build sync handler")
            .expect("sync handler should be created");
        assert_eq!(handler.is_initialized(), false);
    }

    #[test]
    fn test_sync_config_from_ini_valid() {
        let ini_content = r#"
                [gentoo]
                location = /tmp
                auto-sync = true
                sync-type = git
                sync-uri = https://github.com/gentoo-mirror/gentoo.git
            "#;
        let properties = load_properties(ini_content);

        let config = SyncConfig::from_ini(&properties).expect("failed to create SyncConfig");
        assert_eq!(config.location, PathBuf::from("/tmp"));
        assert!(config.auto_sync);
        assert_eq!(
            config.sync_uri,
            Some("https://github.com/gentoo-mirror/gentoo.git".into())
        );
    }

    #[test]
    fn test_sync_config_defaults() {
        let ini_content = r#"
                [gentoo]
                location = /tmp
            "#;
        let properties = load_properties(ini_content);

        let config = SyncConfig::from_ini(&properties).expect("failed to create SyncConfig");
        assert_eq!(config.location, PathBuf::from("/tmp"));
        assert!(config.auto_sync);
        assert!(config.sync_uri.is_none());
    }

    #[test]
    fn test_sync_config_from_ini_invalid_auto_sync() {
        let ini_content = r#"
                [gentoo]
                location = /tmp
                auto-sync = maybe
            "#;
        let properties = load_properties(ini_content);
        assert!(SyncConfig::from_ini(&properties).is_err());
    }
}
