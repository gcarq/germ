use super::error::PhaseExecutionError;
use super::ipc::IpcHandler;
use anyhow::{Context, bail};
use log::warn;
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use std::process::{Child, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

const NATURAL_EXIT_PERIOD: Duration = Duration::from_millis(100);
const SIGTERM_GRACE_PERIOD: Duration = Duration::from_secs(5);
const SIGKILL_WAIT_PERIOD: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Stores the time limits used while stopping an ebuild process.
#[derive(Clone, Copy)]
struct Timeouts {
    /// Time to wait after sending SIGTERM.
    sigterm_grace: Duration,
    /// Time to wait after sending SIGKILL.
    sigkill_wait: Duration,
    /// Process polling interval.
    poll: Duration,
}

#[derive(Clone, Copy)]
struct ProcessState {
    child_reaped: bool,
    group_empty: bool,
}

impl ProcessState {
    const fn is_complete(self) -> bool {
        self.child_reaped && self.group_empty
    }
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            sigterm_grace: SIGTERM_GRACE_PERIOD,
            sigkill_wait: SIGKILL_WAIT_PERIOD,
            poll: POLL_INTERVAL,
        }
    }
}

/// Owns the IPC channel and pgroup lifecycle for an ebuild execution.
///
/// The IPC channel is closed before stopping the process. After sending `DIE`, it stays open so
/// the child can exit normally. Dropping an unfinished execution cleans up its process group.
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
        handler: impl FnOnce(&mut IpcHandler) -> Result<T, PhaseExecutionError>,
    ) -> Result<T, PhaseExecutionError> {
        let ipc = self
            .ipc
            .as_mut()
            .context("ebuild execution IPC is already closed")?;

        match handler(ipc) {
            Ok(data) => {
                self.close_ipc();
                self.complete().map(|()| data)
            }
            // In case of a DIE, we still want to wait for the child to exit and close the
            // IPC channel afterwards
            Err(err @ PhaseExecutionError::Die(_)) => {
                let completion = self.complete();
                self.close_ipc();
                match completion {
                    Ok(()) | Err(PhaseExecutionError::NonZeroExit(_)) => (),
                    Err(cleanup) => {
                        warn!("unable to clean up failed ebuild execution: {cleanup:#}");
                    }
                }
                Err(err)
            }
            Err(err) => {
                self.close_ipc();
                if let Err(cleanup) = self.natural_exit_or_escalate() {
                    warn!("unable to clean up aborted ebuild execution: {cleanup:#}");
                }
                Err(err)
            }
        }
    }

    /// Closes the IPC channel
    fn close_ipc(&mut self) {
        drop(self.ipc.take());
    }

    /// Completes the execution by waiting for natural termination and
    /// verifying that the process group is empty.
    fn complete(&mut self) -> Result<(), PhaseExecutionError> {
        self.natural_exit_or_escalate()?;

        let status = self
            .exit_status
            .ok_or_else(|| anyhow::anyhow!("ebuild process was not reaped"))?;
        let cleanup = self.verify_group_empty();
        if !status.success() {
            if let Err(err) = cleanup {
                warn!("unable to clean up failed ebuild execution: {err:#}");
            }
            return Err(PhaseExecutionError::NonZeroExit(status));
        }
        cleanup?;
        Ok(())
    }

    /// Verifies the process group is empty, killing any remaining processes if needed.
    fn verify_group_empty(&mut self) -> anyhow::Result<()> {
        if self.exit_status.is_none() {
            bail!("ebuild process must be reaped before verifying its process group");
        }
        if process_group_exists(self.process_group)? {
            self.kill_group_and_wait()?;
        }

        self.finalized = true;
        Ok(())
    }

    /// Allows the process to exit naturally before escalating cleanup.
    fn natural_exit_or_escalate(&mut self) -> anyhow::Result<()> {
        if self.wait_for(NATURAL_EXIT_PERIOD, true)?.is_complete() {
            self.finalized = true;
            return Ok(());
        }

        self.escalate()
    }

    /// Stops the process with SIGTERM, then uses SIGKILL if it doesn't exit.
    fn escalate(&mut self) -> anyhow::Result<()> {
        match kill(self.process_group, Signal::SIGTERM) {
            Ok(()) | Err(Errno::ESRCH) => (),
            Err(err) => bail!("unable to signal ebuild process: {err}"),
        };

        let state = self.wait_for(self.timeouts.sigterm_grace, false)?;
        if state.is_complete() {
            self.finalized = true;
            return Ok(());
        }

        self.kill_group_and_wait()
    }

    /// Checks whether the child and its process group have exited.
    fn observe(&mut self) -> anyhow::Result<ProcessState> {
        if self.exit_status.is_none() {
            self.exit_status = self
                .child
                .try_wait()
                .with_context(|| "unable to inspect ebuild process")?;
        }
        let group_empty = !process_group_exists(self.process_group)?;
        Ok(ProcessState {
            child_reaped: self.exit_status.is_some(),
            group_empty,
        })
    }

    /// Kills the process group with SIGKILL and waits for it to exit.
    fn kill_group_and_wait(&mut self) -> anyhow::Result<()> {
        match killpg(self.process_group, Signal::SIGKILL) {
            Ok(()) | Err(Errno::ESRCH) => (),
            Err(err) => bail!("unable to signal ebuild process group: {err}"),
        };

        let is_complete = self
            .wait_for(self.timeouts.sigkill_wait, true)?
            .is_complete();
        self.finalized = true;
        match is_complete {
            true => Ok(()),
            false => bail!("ebuild process group did not exit after SIGKILL"),
        }
    }

    /// Waits for the child, and optionally its process group to exit.
    fn wait_for(
        &mut self,
        timeout: Duration,
        require_group_empty: bool,
    ) -> anyhow::Result<ProcessState> {
        let deadline = Instant::now() + timeout;
        loop {
            let state = self.observe()?;
            let complete = state.child_reaped && (!require_group_empty || state.group_empty);
            if complete || Instant::now() >= deadline {
                return Ok(state);
            }
            thread::sleep(self.timeouts.poll);
        }
    }

    #[cfg(test)]
    pub const fn exit_status(&self) -> Option<ExitStatus> {
        self.exit_status
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

fn process_group_exists(pgroup: Pid) -> anyhow::Result<bool> {
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
        sigterm_grace: Duration::from_millis(100),
        sigkill_wait: Duration::from_secs(1),
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
        let mut execution = spawn_execution("IFS= read -r <&${CHILD_READ_FD}");

        let err = execution
            .run::<()>(|_| Err(PhaseExecutionError::Die(String::new())))
            .unwrap_err();

        assert!(matches!(err, PhaseExecutionError::Die(_)));
        assert!(execution.finalized);
    }

    #[test]
    fn test_run_non_zero_exit() {
        let mut execution = spawn_execution("exit 7");

        let err = execution.run(|_| Ok(())).unwrap_err();

        assert!(
            matches!(err, PhaseExecutionError::NonZeroExit(status) if status.code() == Some(7))
        );
    }

    #[test]
    fn test_run_background_process() {
        let mut execution = spawn_execution("sleep 30 & exit 0");

        let result = execution.run(|_| Ok(42));

        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_natural_exit() {
        let mut execution = spawn_execution("trap 'exit 0' TERM; while :; do :; done");
        thread::sleep(Duration::from_millis(20));

        execution.close_ipc();
        execution.natural_exit_or_escalate().unwrap();

        assert!(execution.exit_status.is_some_and(|status| status.success()));
    }

    #[test]
    fn test_sigterm_escalation() {
        let mut execution = spawn_execution("kill -STOP $$");
        thread::sleep(Duration::from_millis(20));
        let started_at = Instant::now();

        execution.close_ipc();
        execution.natural_exit_or_escalate().unwrap();

        assert!(started_at.elapsed() >= TEST_TIMEOUTS.sigterm_grace);
    }

    #[test]
    fn test_orphaned_process_cleanup() {
        let mut execution = spawn_execution("sleep 30 & exit 0");
        execution.timeouts.sigterm_grace = Duration::from_secs(2);
        thread::sleep(Duration::from_millis(20));
        let started_at = Instant::now();

        execution.close_ipc();
        execution.natural_exit_or_escalate().unwrap();

        assert!(started_at.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn test_sigterm_cleanup_with_orphaned_process() {
        let mut execution = spawn_execution("trap 'exit 0' TERM; sleep 30 & while :; do :; done");
        execution.timeouts.sigterm_grace = Duration::from_secs(2);
        thread::sleep(Duration::from_millis(20));
        let started_at = Instant::now();

        execution.close_ipc();
        execution.natural_exit_or_escalate().unwrap();

        assert!(started_at.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn test_process_group_isolation() {
        let mut first = spawn_execution("kill -STOP $$");
        let mut second = spawn_execution("while :; do :; done");
        thread::sleep(Duration::from_millis(20));

        first.close_ipc();
        first.natural_exit_or_escalate().unwrap();
        let second_is_running = second.child.try_wait().unwrap().is_none();
        second.close_ipc();
        second.natural_exit_or_escalate().unwrap();

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
