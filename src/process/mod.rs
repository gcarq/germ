mod ipc;

use crate::process::ipc::IpcHandler;
use anyhow::{Context, Result, anyhow};
use nix::spawn::{PosixSpawnAttr, PosixSpawnFileActions, posix_spawn};
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{Pid, pipe};
use std::collections::HashMap;
use std::ffi::CString;
use std::mem;
use std::os::fd::AsRawFd;

/// Starts and manages a child process with IPC capabilities.
/// The child process is started with `posix_spawn` and communicates via pipes.
pub struct Process {
    pid: Pid,
    ipc: IpcHandler,
}

impl Process {
    /// Spawns a new process with the given `args` and `env`.
    /// The first element in `args` is expected to be the full path to the binary to execute.
    pub fn new(args: &[&str], env: HashMap<String, String>) -> Result<Self> {
        // IPC pipe for parent -> child
        let (child_reader, parent_writer) = pipe().with_context(|| "unable to create pipe")?;
        // IPC pipe for child -> parent
        let (parent_reader, child_writer) = pipe().with_context(|| "unable to create pipe")?;
        let parent_writer = mem::ManuallyDrop::new(parent_writer);
        let parent_reader = mem::ManuallyDrop::new(parent_reader);

        let mut actions = PosixSpawnFileActions::init()
            .with_context(|| "unable to init posix spawn file actions")?;
        let attrs =
            PosixSpawnAttr::init().with_context(|| "unable to init posix spawn attributes")?;

        actions.add_dup2(child_reader.as_raw_fd(), 3)?;
        actions.add_dup2(child_writer.as_raw_fd(), 6)?;
        actions.add_close(parent_writer.as_raw_fd())?;
        actions.add_close(parent_reader.as_raw_fd())?;

        let binary = args[0];
        let args = args
            .iter()
            .map(|arg| CString::new(*arg))
            .collect::<Result<Vec<CString>, _>>()
            .with_context(|| "unable to setup bash arguments")?;

        let env = env
            .into_iter()
            .map(|(k, v)| CString::new(format!("{k}={v}")))
            .collect::<Result<Vec<CString>, _>>()
            .with_context(|| "unable to setup bash environment")?;

        // SAFETY: We transfer ownership of the pipe ends to Ipc,
        // so they must not be used elsewhere after this call.
        let ipc = unsafe { IpcHandler::new(parent_writer, parent_reader) };

        let pid = posix_spawn(binary, &actions, &attrs, &args, &env)
            .with_context(|| "posix_spawn failed")?;
        Ok(Self { pid, ipc })
    }

    /// Waits for the child process to terminate and returns its exit status.
    pub fn wait(&mut self) -> Result<WaitStatus> {
        waitpid(self.pid, None).with_context(|| anyhow!("unable to wait for process: {}", self.pid))
    }

    /// Shuts down the child process by sending it a SIGTERM signal.
    pub fn shutdown(&mut self) -> Result<()> {
        match waitpid(self.pid, Some(WaitPidFlag::WNOHANG))? {
            WaitStatus::StillAlive => {
                let _ = kill(self.pid, Signal::SIGTERM);
                let _ = waitpid(self.pid, None);
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_ipc() {
        let args = vec![
            "/usr/bin/bash".into(),
            "-c".into(),
            "while read line <&3; do echo \"bash got: $line\" >&6; done".into(),
        ];
        let mut proc = Process::new(&args, HashMap::new()).unwrap();
        proc.ipc.send("hello").unwrap();
        let resp = proc.ipc.recv().unwrap();
        assert_eq!(resp, Some("bash got: hello".into()));
        proc.shutdown().unwrap();
    }
}
