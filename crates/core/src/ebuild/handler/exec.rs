use super::error::ExecutionError;
use super::ipc::IpcHandler;
use anyhow::{Context, Result, bail};
use log::warn;
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use std::process::{Child, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

const GRACE_PERIOD: Duration = Duration::from_secs(5);
const KILL_PERIOD: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Defines timeouts for ebuild execution lifecycle events.
#[derive(Clone, Copy)]
struct Timeouts {
    /// Time until the execution is escalated from SIGTERM to SIGKILL.
    graceful: Duration,
    /// Time until the execution is forcibly killed with SIGKILL.
    forced: Duration,
    /// Process polling interval to check for completion.
    poll: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            graceful: GRACE_PERIOD,
            forced: KILL_PERIOD,
            poll: POLL_INTERVAL,
        }
    }
}

/// Owns the IPC channel and pgroup lifecycle for an ebuild execution.
///
/// The IPC channel is closed before the direct child is reaped or signaled.
/// Dropping an unfinalized execution performs a pgroup cleanup.
pub struct EbuildExecution {
    ipc: Option<IpcHandler>,
    child: Child,
    process_group: Pid,
    exit_status: Option<ExitStatus>,
    finalized: bool,
    timeouts: Timeouts,
}

impl EbuildExecution {
    /// Creates an ebuild execution that owns the given `ipc` channel and direct `child`.
    pub fn new(ipc: IpcHandler, child: Child) -> Self {
        let process_group = Pid::from_raw(child.id() as i32);
        Self {
            ipc: Some(ipc),
            child,
            process_group,
            exit_status: None,
            finalized: false,
            timeouts: Timeouts::default(),
        }
    }

    /// Runs the execution with the given IPC `handler`.
    ///
    /// Data is returned only when the child exits successfully.
    pub fn run<T>(
        &mut self,
        handler: impl FnOnce(&mut IpcHandler) -> Result<T, ExecutionError>,
    ) -> Result<T, ExecutionError> {
        let ipc = self
            .ipc
            .as_mut()
            .context("ebuild execution IPC is already closed")?;
        let result = handler(ipc);
        self.close_ipc();

        match result {
            Ok(data) => self.complete().map(|()| data),
            Err(err) => {
                if let Err(cleanup_err) = self.abort() {
                    warn!("unable to clean up aborted ebuild execution: {cleanup_err:#}");
                }
                Err(err)
            }
        }
    }

    /// Closes the IPC channel
    fn close_ipc(&mut self) {
        drop(self.ipc.take());
    }

    #[cfg(test)]
    pub const fn exit_status(&self) -> Option<ExitStatus> {
        self.exit_status
    }

    /// Completes the execution by reaping the child and verifying that the process group is empty.
    fn complete(&mut self) -> Result<(), ExecutionError> {
        let status = self.reap()?;
        let cleanup = self.verify_group_empty();
        if !status.success() {
            if let Err(cleanup_err) = cleanup {
                warn!("unable to clean up failed ebuild execution: {cleanup_err:#}");
            }
            return Err(ExecutionError::NonZeroExit(status));
        }
        cleanup?;
        Ok(())
    }

    /// Reaps the direct child process and returns its exit status.
    fn reap(&mut self) -> Result<ExitStatus> {
        let status = self
            .child
            .wait()
            .with_context(|| "unable to wait for ebuild process")?;
        self.exit_status = Some(status);
        Ok(status)
    }

    /// Verifies the process group is empty, killing any remaining processes if needed.
    fn verify_group_empty(&mut self) -> Result<()> {
        if self.exit_status.is_none() {
            bail!("ebuild process must be reaped before verifying its process group");
        }
        if process_group_exists(self.process_group)? {
            self.kill_group_and_wait()?;
        }

        self.finalized = true;
        Ok(())
    }

    /// Aborts the execution by signaling the process group and waiting for it to exit.
    fn abort(&mut self) -> Result<()> {
        match kill(self.process_group, Signal::SIGTERM) {
            Ok(()) | Err(Errno::ESRCH) => (),
            Err(err) => bail!("unable to signal ebuild process: {err}"),
        };

        let (child_reaped, group_empty) =
            self.wait_for(self.timeouts.graceful, |(child_reaped, _)| child_reaped)?;
        if child_reaped && group_empty {
            self.finalized = true;
            return Ok(());
        }

        self.kill_group_and_wait()
    }

    /// Observes the execution state by checking if the child has exited
    /// and if the process group is empty.
    ///
    /// Returns a tuple of (child_reaped, group_empty)
    fn observe(&mut self) -> Result<(bool, bool)> {
        if self.exit_status.is_none() {
            self.exit_status = self
                .child
                .try_wait()
                .with_context(|| "unable to inspect ebuild process")?;
        }
        let group_empty = !process_group_exists(self.process_group)?;
        Ok((self.exit_status.is_some(), group_empty))
    }

    /// Kills the process group with SIGKILL and waits for it to exit.
    fn kill_group_and_wait(&mut self) -> Result<()> {
        match killpg(self.process_group, Signal::SIGKILL) {
            Ok(()) | Err(Errno::ESRCH) => (),
            Err(err) => bail!("unable to signal ebuild process group: {err}"),
        };

        let (child_reaped, group_empty) = self
            .wait_for(self.timeouts.forced, |(child_reaped, group_empty)| {
                child_reaped && group_empty
            })?;
        if child_reaped && group_empty {
            self.finalized = true;
            return Ok(());
        }

        self.finalized = true;
        bail!("ebuild process group did not exit after SIGKILL");
    }

    /// Waits for the execution to reach a desired state specified by `done` or times out.
    fn wait_for(
        &mut self,
        timeout: Duration,
        done: impl Fn((bool, bool)) -> bool,
    ) -> Result<(bool, bool)> {
        let deadline = Instant::now() + timeout;
        loop {
            let observation = self.observe()?;
            if done(observation) || Instant::now() >= deadline {
                return Ok(observation);
            }
            thread::sleep(self.timeouts.poll);
        }
    }
}

impl Drop for EbuildExecution {
    fn drop(&mut self) {
        self.close_ipc();
        if self.finalized {
            return;
        }
        if let Err(err) = self.kill_group_and_wait() {
            warn!("unable to clean up abandoned ebuild execution: {err:#}");
        }
    }
}

fn process_group_exists(pgroup: Pid) -> Result<bool> {
    match killpg(pgroup, None) {
        Ok(()) | Err(Errno::EPERM) => Ok(true),
        Err(Errno::ESRCH) => Ok(false),
        Err(err) => Err(err).with_context(|| "unable to inspect ebuild process group"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::BASH_BINARY_PATH;
    use crate::types::FxHashMap;

    const TEST_TIMEOUTS: Timeouts = Timeouts {
        graceful: Duration::from_millis(100),
        forced: Duration::from_secs(1),
        poll: Duration::from_millis(5),
    };

    fn spawn_execution(script: &str) -> EbuildExecution {
        let args = vec!["-c".into(), script.into()];
        let (ipc, child) =
            IpcHandler::spawn(BASH_BINARY_PATH, &args, &FxHashMap::default()).unwrap();
        let mut execution = EbuildExecution::new(ipc, child);
        execution.timeouts = TEST_TIMEOUTS;
        execution
    }

    #[test]
    fn test_run_success() {
        let mut execution = spawn_execution("exit 0");

        execution.run(|_| Ok(())).unwrap();

        assert!(execution.exit_status.is_some_and(|status| status.success()));
    }

    #[test]
    fn test_run_die() {
        let mut execution = spawn_execution("while :; do :; done");

        let err = execution
            .run::<()>(|_| Err(ExecutionError::Die("fatal error".into())))
            .unwrap_err();

        assert!(matches!(err, ExecutionError::Die(message) if message == "fatal error"));
    }

    #[test]
    fn test_run_non_zero_exit() {
        let mut execution = spawn_execution("exit 7");

        let err = execution.run(|_| Ok(())).unwrap_err();

        assert!(matches!(err, ExecutionError::NonZeroExit(status) if status.code() == Some(7)));
    }

    #[test]
    fn test_run_background_process() {
        let mut execution = spawn_execution("sleep 30 & exit 0");

        let result = execution.run(|_| Ok(42));

        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_abort_graceful() {
        let mut execution = spawn_execution("trap 'exit 0' TERM; while :; do :; done");
        thread::sleep(Duration::from_millis(20));

        execution.close_ipc();
        execution.abort().unwrap();

        assert!(execution.exit_status.is_some_and(|status| status.success()));
    }

    #[test]
    fn test_abort_escalation() {
        let mut execution = spawn_execution("kill -STOP $$");
        thread::sleep(Duration::from_millis(20));
        let started_at = Instant::now();

        execution.close_ipc();
        execution.abort().unwrap();

        assert!(started_at.elapsed() >= TEST_TIMEOUTS.graceful);
    }

    #[test]
    fn test_abort_orphaned_process() {
        let mut execution = spawn_execution("sleep 30 & exit 0");
        execution.timeouts.graceful = Duration::from_secs(2);
        thread::sleep(Duration::from_millis(20));
        let started_at = Instant::now();

        execution.close_ipc();
        execution.abort().unwrap();

        assert!(started_at.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn test_abort_graceful_orphaned_process() {
        let mut execution = spawn_execution("trap 'exit 0' TERM; sleep 30 & while :; do :; done");
        execution.timeouts.graceful = Duration::from_secs(2);
        thread::sleep(Duration::from_millis(20));
        let started_at = Instant::now();

        execution.close_ipc();
        execution.abort().unwrap();

        assert!(started_at.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn test_abort_process_group_isolation() {
        let mut first = spawn_execution("kill -STOP $$");
        let mut second = spawn_execution("while :; do :; done");
        thread::sleep(Duration::from_millis(20));

        first.close_ipc();
        first.abort().unwrap();
        let second_is_running = second.child.try_wait().unwrap().is_none();
        second.close_ipc();
        second.abort().unwrap();

        assert!(second_is_running);
    }

    #[test]
    fn test_drop_cleanup() {
        let execution = spawn_execution("sleep 30 & wait");
        let process_group = execution.process_group;

        drop(execution);

        assert!(!process_group_exists(process_group).unwrap());
    }
}
