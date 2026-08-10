//! This module defines the synchronization mechanism for repositories.
//! Currently only `git` is supported.

mod git;

use self::git::GitSyncHandler;
use crate::types::FxHashMap;
use anyhow::{Context, anyhow, bail};
use std::fmt;
use std::path::PathBuf;

enum SyncType {
    Git,
}

impl SyncType {
    fn new(sync_type: &str) -> anyhow::Result<Self> {
        match sync_type {
            "git" => Ok(SyncType::Git),
            _ => bail!("unsupported sync-type: '{sync_type}'"),
        }
    }
}

/// Configuration for the synchronization mechanism.
/// This struct holds common options that are used for all [`SyncType`],
/// such as `location`, `auto_sync`, `sync_uri`, etc.
#[derive(Debug)]
struct SyncConfig {
    // Absolute path to the repository location on the local filesystem.
    location: PathBuf,
    sync_uri: String,
}

impl SyncConfig {
    fn from_ini(properties: &FxHashMap<String, String>) -> anyhow::Result<Self> {
        let location = properties
            .get("location")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("missing required 'location' property"))?;

        let Some(sync_uri) = properties.get("sync-uri").map(ToOwned::to_owned) else {
            bail!("missing required 'sync-uri' property");
        };

        Ok(SyncConfig { location, sync_uri })
    }
}

/// Builds a synchronization handler based on the provided INI `properties`.
/// The `sync-type` property is used to determine which synchronization mechanism to use.
pub fn build_sync_handler(
    properties: &FxHashMap<String, String>,
) -> anyhow::Result<Option<Box<dyn SyncHandler>>> {
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
pub trait SyncHandler: fmt::Debug + Send + Sync {
    /// Creates a new instance of the `SyncHandler` based on the provided INI `properties`
    /// for this repository coming from `repos.conf`.
    fn new(properties: &FxHashMap<String, String>) -> anyhow::Result<Self>
    where
        Self: Sized;

    /// Conditionally syncs the repository using either `init` or `update`.
    fn sync(&self) -> anyhow::Result<()> {
        match self.is_initialized() {
            true => self.update(),
            false => self.init(),
        }
    }

    /// Checks if the repository is already initialized.
    /// This should be used to determine whether to call `init` or `update` in the `sync` method.
    fn is_initialized(&self) -> bool;

    /// Initializes and downloads the repository.
    fn init(&self) -> anyhow::Result<()>;

    /// Updates an existing repository.
    fn update(&self) -> anyhow::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ini::Ini;

    fn load_properties(ini_content: &str) -> FxHashMap<String, String> {
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
                sync-uri = https://github.com/gentoo-mirror/gentoo.git
            "#;
        let properties = load_properties(ini_content);

        let handler = build_sync_handler(&properties)
            .expect("failed to build sync handler")
            .expect("sync handler should be created");
        assert!(!handler.is_initialized());
    }
}
