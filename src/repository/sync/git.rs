use crate::repository::sync::{SyncConfig, SyncHandler};
use anyhow::{Context, Result};
use ini::Properties;

pub struct GitSyncHandler {
    config: SyncConfig,
    // Defaults to 1 (only the newest commit). If set to 0, the depth is unlimited.
    pub clone_depth: usize,
    // If set to 0, the depth is unlimited. Defaults to 0.
    pub sync_depth: usize,
}

impl GitSyncHandler {
    // Additional methods specific to Git synchronization can be added here.
}

impl SyncHandler for GitSyncHandler {
    fn new(properties: &Properties) -> Result<Self>
    where
        Self: Sized,
    {
        let config = SyncConfig::from_ini(properties)?;
        let clone_depth = properties
            .get("clone-depth")
            .map(|s| s.parse::<usize>())
            .transpose()
            .with_context(|| "invalid clone-depth value")?
            .unwrap_or(1);

        let sync_depth = properties
            .get("sync-depth")
            .map(|s| s.parse::<usize>())
            .transpose()
            .with_context(|| "invalid sync-depth value")?
            .unwrap_or(0);

        Ok(Self {
            config,
            clone_depth,
            sync_depth,
        })
    }

    fn is_initialized(&self) -> bool {
        self.config.location.join(".git").exists()
    }

    fn init(&self) -> Result<()> {
        println!(
            "Initializing git repository at {}",
            self.config.location.display()
        );
        Ok(())
    }

    fn update(&self) -> Result<()> {
        println!(
            "Updating git repository at {}",
            self.config.location.display()
        );
        Ok(())
    }
}
