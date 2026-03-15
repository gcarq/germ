pub mod ipc;

use crate::process::ipc::IpcHandler;
use crate::types::FxHashMap;
use anyhow::{Context, Result, anyhow};
use log::trace;
use nix::spawn::{PosixSpawnAttr, PosixSpawnFileActions, posix_spawn};
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;
use std::ffi::CString;
use std::os::fd::RawFd;

/// Starts and manages a child process with IPC capabilities.
/// The child process is started with `posix_spawn` and communicates via pipes.
pub struct Process {
    pub pid: Pid,
    pub ipc: Option<IpcHandler>,
}

impl Process {
    /// Uses `posix_spawn` to create a subprocess with the given `command` and `env`.
    ///
    /// The first element in `command` is expected to be the absolute path to the binary to execute.
    pub fn new(command: &[String], env: &FxHashMap<String, String>) -> Result<Self> {
        let actions =
            PosixSpawnFileActions::init().with_context(|| "unable to init posix_spawn actions")?;
        let pid = Self::spawn(command, env, &actions)?;
        Ok(Self { pid, ipc: None })
    }

    /// Uses `posix_spawn` to create a subprocess with the given `command` and `env` with IPC
    /// capabilities using [`IpcHandler`].
    ///
    /// `child_channel` holds the (read, write) file descriptors the child should will use.
    ///
    /// The first element in `command` is expected to be the absolute path to the binary to execute.
    pub fn with_ipc(
        command: &[String],
        env: &FxHashMap<String, String>,
        child_channel: (RawFd, RawFd),
    ) -> Result<Self> {
        let (ipc, pid) =
            IpcHandler::new(child_channel, |actions| Self::spawn(command, env, &actions))
                .with_context(|| "unable to setup IPC handler")?;
        Ok(Self {
            pid,
            ipc: Some(ipc),
        })
    }

    /// Checks if the child process is still alive.
    pub fn is_alive(&self) -> bool {
        matches!(
            waitpid(self.pid, Some(WaitPidFlag::WNOHANG)),
            Ok(WaitStatus::StillAlive)
        )
    }

    /// Waits for the child process to terminate and returns its exit status.
    pub fn wait(&mut self) -> Result<WaitStatus> {
        waitpid(self.pid, None).with_context(|| anyhow!("unable to wait for process: {}", self.pid))
    }

    /// Stops the child process by sending it a `SIGTERM` signal.
    pub fn stop(&mut self) -> Result<()> {
        if let WaitStatus::StillAlive = waitpid(self.pid, Some(WaitPidFlag::WNOHANG))? {
            let _ = kill(self.pid, Signal::SIGTERM);
            let _ = waitpid(self.pid, None);
        }
        Ok(())
    }

    /// Helper function to spawn a sub process with the given `args` and `env` and `actions`.
    fn spawn(
        command: &[String],
        env: &FxHashMap<String, String>,
        actions: &PosixSpawnFileActions,
    ) -> Result<Pid> {
        trace!("Spawning process: '{}' ...", command.join(" "),);
        let binary = command
            .first()
            .ok_or_else(|| anyhow!("no arguments provided to spawn process"))?
            .as_str();

        let args = command
            .iter()
            .map(|arg| CString::new(arg.clone()))
            .collect::<Result<Vec<CString>, _>>()
            .with_context(|| "unable to setup bash arguments")?;

        let env = env
            .iter()
            .map(|(k, v)| CString::new(format!("{k}={v}")))
            .collect::<Result<Vec<CString>, _>>()
            .with_context(|| "unable to setup environment")?;

        let attrs = PosixSpawnAttr::init().with_context(|| "unable to init posix_spawn attrs")?;
        posix_spawn(binary, actions, &attrs, &args, &env).with_context(|| "posix_spawn failed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ipc::{ChildToParentMsg, ParentToChildMsg};

    struct ParentTestMsg(String);
    impl ParentToChildMsg for ParentTestMsg {
        fn into_bytes(self) -> Vec<u8> {
            self.0.into_bytes()
        }
    }

    struct ChildTestMsg(pub String);

    impl ChildToParentMsg for ChildTestMsg {
        fn from_bytes(bytes: &[u8]) -> Result<Self> {
            let s = String::from_utf8(bytes.to_owned())
                .with_context(|| "invalid UTF-8 in child message")?
                .clone();
            Ok(Self(s))
        }
    }

    #[test]
    fn test_process_new() {
        let args = vec!["/usr/bin/sleep".into(), "infinity".into()];
        let mut proc = Process::new(&args, &FxHashMap::default()).unwrap();
        assert!(proc.is_alive(), "process should be alive");
        proc.stop().unwrap();
        assert!(!proc.is_alive(), "process should be stopped");
    }

    #[test]
    fn test_process_with_ipc() {
        let args = vec![
            "/usr/bin/bash".into(),
            "-c".into(),
            r#"IFS= read -r response <&10 || exit 1
            printf '%s\4' "${response}" >&11 || exit 1
            printf '%s\4' "${TEST_ENV}" >&11 || exit 1
            sleep 30"#
                .into(),
        ];
        let env: FxHashMap<String, String> =
            FxHashMap::from_iter([("TEST_ENV".into(), "42".into())]);
        let mut proc = Process::with_ipc(&args, &env, (10, 11)).unwrap();
        let ipc = proc.ipc.as_mut().unwrap();

        ipc.send(ParentTestMsg("sync".into())).unwrap();
        let resp = ipc.recv::<ChildTestMsg>().unwrap().unwrap();
        assert_eq!(resp.0, "sync", "expected echoed line");

        let resp = ipc.recv::<ChildTestMsg>().unwrap().unwrap();
        assert_eq!(resp.0, "42", "expected value from env variable");

        proc.stop().unwrap();
    }
}
