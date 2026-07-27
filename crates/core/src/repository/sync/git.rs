use crate::consts::GIT_BINARY_PATH;
use crate::repository::sync::{SyncConfig, SyncHandler};
use crate::types::FxHashMap;
use anyhow::{Context, Result, anyhow, bail};
use log::{debug, info};
use std::collections::HashMap;
use std::process::{Command, Output, Stdio};

#[derive(Debug)]
pub struct GitSyncHandler {
    config: SyncConfig,
    // If set to 0, the depth is unlimited. Defaults to 1.
    pub clone_depth: usize,
    // If set to 0, the depth is unlimited. Defaults to 1.
    pub sync_depth: usize,
}

/// Helper function to create a non-interactive git `Command`
fn git_command() -> Command {
    let mut command = Command::new(GIT_BINARY_PATH);
    command.env("GIT_TERMINAL_PROMPT", "0").stdin(Stdio::null());
    command
}

impl SyncHandler for GitSyncHandler {
    fn new(properties: &FxHashMap<String, String>) -> Result<Self>
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
        info!("Cloning from '{}'", self.config.sync_uri);
        let mut command = git_command();
        command.arg("clone");
        if self.clone_depth > 0 {
            command.arg("--depth").arg(self.clone_depth.to_string());
        }
        command
            .arg(&self.config.sync_uri)
            .arg(&self.config.location);

        let output = Self::execute(command)?;
        Self::log_output(&output);
        Ok(())
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

        self.set_or_add_origin(&self.config.sync_uri, &env)?;

        let mut command_opts = Vec::new();
        if self.sync_depth > 0 {
            command_opts.extend(["--depth".into(), self.sync_depth.to_string()]);
            // For shallow fetch, unreachable objects may need to be pruned
            // manually, in order to prevent automatic git gc calls from
            // eventually failing (see bug 599008).
            self.prune_shallow_repository(&env)
                .with_context(|| anyhow!("git gc failed at {}", self.config.location.display()))?;
        }

        self.fetch_remote("origin", &command_opts, &env)?;
        let remote_branch = self
            .resolve_remote_branch(&env)
            .with_context(|| "unable to resolve remote branch to reset to")?;
        self.reset_hard(&remote_branch, &env)?;
        Ok(())
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

    /// Executes the given `command` and returns the `Output`.
    fn execute(mut command: Command) -> Result<Output> {
        let output = command
            .output()
            .with_context(|| format!("failed to execute {}", Self::command_repr(&command)))?;
        if output.status.success() {
            Ok(output)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let msg = anyhow!(
                "{} failed with exit code {}\n{}",
                Self::command_repr(&command),
                output.status,
                stderr.trim_end(),
            );
            Err(msg)
        }
    }

    /// Returns a string representation of the command and its arguments for logging purposes.
    fn command_repr(command: &Command) -> String {
        std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Logs the output in `stderr` as info log.
    /// git logs
    fn log_output(output: &Output) {
        let msg = String::from_utf8_lossy(&output.stderr);
        let msg = msg.trim_end();
        if !msg.is_empty() {
            info!("{msg}");
        }
    }

    /// Updates the origin to the given `sync_uri`
    fn set_or_add_origin(&self, sync_uri: &str, env: &HashMap<String, String>) -> Result<()> {
        let mut set_url = git_command();
        set_url
            .arg("remote")
            .arg("set-url")
            .arg("origin")
            .arg(sync_uri)
            .current_dir(&self.config.location)
            .envs(env);

        if Self::execute(set_url).is_ok() {
            debug!("Set origin remote to {sync_uri}");
            return Ok(());
        }

        // In case the remote doesn't exist yet
        let mut add = Command::new(GIT_BINARY_PATH);
        add.arg("remote")
            .arg("add")
            .arg("origin")
            .arg(sync_uri)
            .current_dir(&self.config.location)
            .envs(env);
        Self::execute(add).with_context(|| "failed to set or add origin remote")?;
        debug!("Set origin remote to {sync_uri}");
        Ok(())
    }

    /// Prunes unreachable objects from the repository to prevent git gc from failing due to too
    /// many loose objects.
    fn prune_shallow_repository(&self, env: &HashMap<String, String>) -> Result<()> {
        let mut command = git_command();
        command
            .arg("-c")
            .arg("gc.autodetach=false")
            .arg("gc")
            .arg("--auto")
            .current_dir(&self.config.location)
            .envs(env);
        Self::execute(command)?;
        Ok(())
    }

    /// Resolves the git remote branch that should be synced into the current checkout.
    fn resolve_remote_branch(&self, env: &HashMap<String, String>) -> Result<String> {
        match self.rev_parse_abbrev_ref("@{upstream}", env) {
            Ok(branch) => Ok(branch),
            Err(upstream_err) => self
                .rev_parse_abbrev_ref("origin/HEAD", env)
                .with_context(|| {
                    format!("unable to resolve upstream ({upstream_err}) or origin/HEAD")
                }),
        }
    }

    fn rev_parse_abbrev_ref(
        &self,
        revision: &str,
        env: &HashMap<String, String>,
    ) -> Result<String> {
        let mut command = git_command();
        command
            .arg("rev-parse")
            .arg("--abbrev-ref")
            .arg("--symbolic-full-name")
            .arg(revision)
            .current_dir(&self.config.location)
            .envs(env);
        let output = Self::execute(command)?;
        let branch = String::from_utf8(output.stdout)
            .with_context(|| "invalid UTF-8 in git rev-parse output")?
            .trim()
            .to_owned();
        match branch.is_empty() {
            true => bail!("git rev-parse returned an empty branch"),
            false => Ok(branch),
        }
    }

    /// Fetches the given `remote` with the specified git `cmd_opts` and `env`.
    fn fetch_remote(
        &self,
        remote: &str,
        cmd_opts: &[String],
        env: &HashMap<String, String>,
    ) -> Result<()> {
        let mut command = git_command();
        command
            .arg("fetch")
            .arg(remote)
            .args(cmd_opts)
            .current_dir(&self.config.location)
            .envs(env);
        let output = Self::execute(command)?;
        Self::log_output(&output);
        Ok(())
    }

    /// Performs a hard reset of the current branch to the specified `target` revision.
    fn reset_hard(&self, target: &str, env: &HashMap<String, String>) -> Result<()> {
        let mut command = git_command();
        command
            .arg("reset")
            .arg("--hard")
            .arg(target)
            .current_dir(&self.config.location)
            .envs(env);
        let output = Self::execute(command)?;
        Self::log_output(&output);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::test_support::RepositoryFixture;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn properties(location: &Path, sync_uri: &str) -> FxHashMap<String, String> {
        let mut properties = FxHashMap::default();
        properties.insert("location".into(), location.to_string_lossy().into_owned());
        properties.insert("sync-type".into(), "git".into());
        properties.insert("sync-uri".into(), sync_uri.into());
        properties
    }

    fn run(command: &mut Command) -> Output {
        let output = command.output().expect("failed to execute git");
        assert!(
            output.status.success(),
            "git command failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn git_in(path: &Path, args: &[&str]) -> Output {
        let mut command = git_command();
        command.args(args).current_dir(path);
        run(&mut command)
    }

    fn commit_all(path: &Path, message: &str) {
        git_in(path, &["add", "."]);
        git_in(
            path,
            &[
                "-c",
                "user.name=Germ Test",
                "-c",
                "user.email=germ@example.invalid",
                "commit",
                "-m",
                message,
            ],
        );
    }

    fn create_origin(root: &Path, repo_name: &str, marker: &str) -> (PathBuf, PathBuf, String) {
        let bare = root.join("origin.git");
        let work = root.join("work");
        fs::create_dir_all(&work).unwrap();

        let mut command = git_command();
        command.args(["init", "--bare", bare.to_str().unwrap()]);
        run(&mut command);
        git_in(&work, &["init"]);
        git_in(&work, &["checkout", "-B", "main"]);
        RepositoryFixture::with_location(&work, repo_name)
            .categories(["app-misc"])
            .write()
            .unwrap();
        fs::write(work.join("MARKER"), marker).unwrap();
        commit_all(&work, "initial");
        let origin_url = format!("file://{}", bare.display());
        git_in(&work, &["remote", "add", "origin", &origin_url]);
        git_in(&work, &["push", "-u", "origin", "main"]);
        let mut set_head = Command::new(GIT_BINARY_PATH);
        set_head
            .arg("--git-dir")
            .arg(&bare)
            .arg("symbolic-ref")
            .arg("HEAD")
            .arg("refs/heads/main");
        run(&mut set_head);
        (bare, work, origin_url)
    }

    #[test]
    fn test_parses_default_depth_values() {
        let temp = tempfile::Builder::new().tempdir().unwrap();
        let properties = properties(&temp.path().join("repo"), "file:///tmp/origin.git");

        let handler = GitSyncHandler::new(&properties).unwrap();
        assert_eq!(handler.clone_depth, 1);
        assert_eq!(handler.sync_depth, 1);
    }

    #[test]
    fn test_clone_from_local_origin_creates_destination() {
        let temp = tempfile::Builder::new().tempdir().unwrap();
        let (_bare, _work, origin_url) = create_origin(temp.path(), "gentoo", "initial");
        let destination = temp.path().join("gentoo");
        let mut properties = properties(&destination, &origin_url);
        properties.insert("clone-depth".into(), "1".into());

        let handler = GitSyncHandler::new(&properties).unwrap();
        handler.sync().unwrap();

        assert!(destination.join(".git").exists());
        let output = git_in(&destination, &["rev-parse", "--is-shallow-repository"]);
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "true");
    }

    #[test]
    fn test_clone_depth_zero_uses_full_history() {
        let temp = tempfile::Builder::new().tempdir().unwrap();
        let (_bare, _work, origin_url) = create_origin(temp.path(), "gentoo", "initial");
        let destination = temp.path().join("gentoo");
        let mut properties = properties(&destination, &origin_url);
        properties.insert("clone-depth".into(), "0".into());

        let handler = GitSyncHandler::new(&properties).unwrap();
        handler.sync().unwrap();

        let output = git_in(&destination, &["rev-parse", "--is-shallow-repository"]);
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "false");
    }

    #[test]
    fn test_update_fetches_and_hard_resets_to_origin() {
        let temp = tempfile::Builder::new().tempdir().unwrap();
        let (_bare, work, origin_url) = create_origin(temp.path(), "gentoo", "initial");
        let destination = temp.path().join("gentoo");
        let mut properties = properties(&destination, &origin_url);
        properties.insert("clone-depth".into(), "0".into());
        properties.insert("sync-depth".into(), "0".into());
        let handler = GitSyncHandler::new(&properties).unwrap();
        handler.sync().unwrap();

        fs::write(destination.join("MARKER"), "local commit").unwrap();
        commit_all(&destination, "local commit");

        fs::write(work.join("MARKER"), "remote update").unwrap();
        commit_all(&work, "remote update");
        git_in(&work, &["push", "origin", "main"]);

        handler.sync().unwrap();

        assert_eq!(
            fs::read_to_string(destination.join("MARKER")).unwrap(),
            "remote update"
        );
        let head = git_in(&destination, &["rev-parse", "HEAD"]);
        let origin_head = git_in(&destination, &["rev-parse", "origin/main"]);
        assert_eq!(head.stdout, origin_head.stdout);
    }

    #[test]
    fn test_update_replaces_existing_origin_url() {
        let temp = tempfile::Builder::new().tempdir().unwrap();
        let origin_one_root = temp.path().join("one");
        let origin_two_root = temp.path().join("two");
        fs::create_dir_all(&origin_one_root).unwrap();
        fs::create_dir_all(&origin_two_root).unwrap();
        let (_bare_one, _work_one, origin_one_url) =
            create_origin(&origin_one_root, "gentoo", "one");
        let (_bare_two, _work_two, origin_two_url) =
            create_origin(&origin_two_root, "gentoo", "two");
        let destination = temp.path().join("gentoo");

        let mut initial_properties = properties(&destination, &origin_one_url);
        initial_properties.insert("clone-depth".into(), "0".into());
        GitSyncHandler::new(&initial_properties)
            .unwrap()
            .sync()
            .unwrap();

        let mut replacement_properties = properties(&destination, &origin_two_url);
        replacement_properties.insert("sync-depth".into(), "0".into());
        GitSyncHandler::new(&replacement_properties)
            .unwrap()
            .sync()
            .unwrap();

        let output = git_in(&destination, &["remote", "get-url", "origin"]);
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            origin_two_url
        );
        assert_eq!(
            fs::read_to_string(destination.join("MARKER")).unwrap(),
            "two"
        );
    }
}
