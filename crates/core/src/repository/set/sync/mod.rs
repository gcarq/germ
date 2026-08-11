//! This module defines the synchronization mechanism for repositories.
//! Currently only `git` is supported.

mod git;

use self::git::GitSyncHandler;
use crate::types::FxHashMap;
use anyhow::{Context, anyhow, bail};
use log::{debug, info};
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
    auto_sync: bool,
}

impl SyncConfig {
    fn from_ini(properties: &FxHashMap<String, String>) -> anyhow::Result<Self> {
        let location = properties
            .get("location")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("missing required 'location' property"))?;

        let sync_uri = match properties.get("sync-uri") {
            Some(uri) => uri.to_owned(),
            None => bail!("missing required 'sync-uri' property"),
        };

        let auto_sync = match properties.get("auto-sync") {
            Some(value) => match value.as_str() {
                "true" | "yes" => true,
                "false" | "no" => false,
                _ => bail!("invalid 'auto-sync' value: '{value}'"),
            },
            None => true, // Default to true if not specified
        };

        Ok(SyncConfig {
            location,
            sync_uri,
            auto_sync,
        })
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
    ///
    /// If `force` is true, the sync will be performed regardless of the `auto_sync` setting.
    fn sync(&self, name: &str, force: bool) -> anyhow::Result<()> {
        if !self.auto_sync() && !force {
            debug!("auto-sync disabled for {name}");
            return Ok(());
        }

        info!("Syncing repository '{name}'");
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

    fn auto_sync(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ini::Ini;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Default)]
    struct TestSyncHandler {
        auto_sync: bool,
        sync_calls: AtomicUsize,
    }

    impl SyncHandler for TestSyncHandler {
        fn new(_properties: &FxHashMap<String, String>) -> anyhow::Result<Self> {
            Ok(Default::default())
        }

        fn is_initialized(&self) -> bool {
            true
        }

        fn init(&self) -> anyhow::Result<()> {
            self.sync_calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn update(&self) -> anyhow::Result<()> {
            self.sync_calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn auto_sync(&self) -> bool {
            self.auto_sync
        }
    }

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
    fn test_sync_respects_auto_sync() {
        let handler = TestSyncHandler {
            auto_sync: false,
            sync_calls: AtomicUsize::new(0),
        };
        handler.sync("gentoo", false).unwrap();
        handler.sync("gentoo", true).unwrap();
        assert_eq!(handler.sync_calls.load(Ordering::Relaxed), 1);
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
