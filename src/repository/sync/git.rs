use crate::consts::GIT_BINARY_PATH;
use crate::repository::sync::{SyncConfig, SyncHandler};
use anyhow::{Context, Result, anyhow};
use log::info;
use std::collections::HashMap;
use std::process::Command;

#[derive(Debug)]
pub struct GitSyncHandler {
    config: SyncConfig,
    // If set to 0, the depth is unlimited. Defaults to 1.
    pub clone_depth: usize,
    // If set to 0, the depth is unlimited. Defaults to 1.
    pub sync_depth: usize,
}

impl SyncHandler for GitSyncHandler {
    fn new(properties: &HashMap<String, String>) -> Result<Self>
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
            .unwrap_or(1);

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
        info!(
            "Initializing git repository at {}",
            self.config.location.display()
        );
        todo!("handle clone from sync_uri");
    }

    fn update(&self) -> Result<()> {
        info!(
            "Updating git repository at {}",
            self.config.location.display()
        );

        // We set `GIT_CEILING_DIRECTORIES` to the ancestor directories of the repository location
        // to prevent git from searching for a .git directory in parent directories
        let env = HashMap::from([(
            "GIT_CEILING_DIRECTORIES".into(),
            self.ceiling_directories()?.join(":"),
        )]);

        let mut command_opts = Vec::new();

        if self.sync_depth > 0 {
            command_opts.extend(["--depth".into(), self.sync_depth.to_string()]);
            // For shallow fetch, unreachable objects may need to be pruned
            // manually, in order to prevent automatic git gc calls from
            // eventually failing (see bug 599008).
            self.prune_shallow_repository(&env)
                .with_context(|| anyhow!("git gc failed at {}", self.config.location.display()))?;
        }

        let remote_branch = self
            .resolve_remote_branch(&env)
            .with_context(|| "unable to resolve remote")?;
        let (remote, _branch) = remote_branch
            .split_once('/')
            .ok_or_else(|| anyhow!("invalid remote branch format: '{remote_branch}'"))?;

        self.fetch_remote(remote, &command_opts, &env)?;

        todo!("handle merge/rebase");
    }
}

impl GitSyncHandler {
    /// Returns a list of ancestor directories of the repository location
    /// to be used as `GIT_CEILING_DIRECTORIES`.
    fn ceiling_directories(&self) -> Result<Vec<&str>> {
        self.config
            .location
            .ancestors()
            .map(|p| {
                p.to_str()
                    .ok_or_else(|| anyhow!("invalid UTF-8 in path '{}'", p.display()))
            })
            .collect::<Result<Vec<_>>>()
    }

    /// Prunes unreachable objects from the repository to prevent git gc from failing due to too
    /// many loose objects.
    fn prune_shallow_repository(&self, env: &HashMap<String, String>) -> Result<()> {
        let exit_status = Command::new(GIT_BINARY_PATH)
            .arg("-c")
            .arg("gc.autodetach=false")
            .arg("gc")
            .arg("--auto")
            .current_dir(&self.config.location)
            .envs(env)
            .spawn()
            .and_then(|mut child| child.wait())
            .with_context(|| "failed to execute git rev-parse")?;
        match exit_status.success() {
            true => Ok(()),
            false => Err(anyhow!("git gc failed with exit code {exit_status}")),
        }
    }

    /// Resolves the git remote branch that the current branch is tracking.
    fn resolve_remote_branch(&self, env: &HashMap<String, String>) -> Result<String> {
        let mut output = Command::new(GIT_BINARY_PATH)
            .arg("rev-parse")
            .arg("--abbrev-ref")
            .arg("--symbolic-full-name")
            .arg("@{upstream}")
            .current_dir(&self.config.location)
            .envs(env)
            .output()
            .with_context(|| "failed to execute git rev-parse")?;
        if !output.status.success() {
            return Err(anyhow!(
                "git rev-parse failed with exit code {}",
                output.status
            ));
        }
        output.stdout.pop();
        String::from_utf8(output.stdout).with_context(|| "invalid UTF-8 in git rev-parse output")
    }

    /// Fetches the given `remote` with the specified git `cmd_opts` and `env`.
    fn fetch_remote(
        &self,
        remote: &str,
        cmd_opts: &[String],
        env: &HashMap<String, String>,
    ) -> Result<()> {
        let exit_status = Command::new(GIT_BINARY_PATH)
            .arg("fetch")
            .arg(remote)
            .args(cmd_opts)
            .current_dir(&self.config.location)
            .envs(env)
            .spawn()
            .and_then(|mut child| child.wait())
            .with_context(|| "failed to fetch remote")?;
        match exit_status.success() {
            true => Ok(()),
            false => Err(anyhow!("git fetch failed with exit code {exit_status}")),
        }
    }
}
