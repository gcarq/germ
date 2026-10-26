use std::process::ExitStatus;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use log::warn;
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use tokio::process::Child;
use tokio::time;

use super::error::PhaseExecutionError;
use super::ipc::IpcHandler;

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

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            sigterm_grace: SIGTERM_GRACE_PERIOD,
            sigkill_wait: SIGKILL_WAIT_PERIOD,
            poll: POLL_INTERVAL,
        }
    }
}

/// Controls how long [`EbuildExecution::wait_for`] waits for the process.
#[derive(Clone, Copy)]
enum WaitUntil {
    /// Wait only for the direct child to exit.
    ChildExited,
    /// Wait for the child to exit and the process group to be gone.
    GroupGone,
}

/// Owns the IPC channel and pgroup lifecycle for an ebuild execution.
///
/// The IPC channel is closed before stopping the process. After sending `DIE`, it stays open so
/// the child can exit normally. Dropping an unfinished execution cleans up its process group.
pub struct EbuildExecution {
    ipc: Option<IpcHandler>,
    child: Child,
    process_group: Pid,
    finalized: bool,
    timeouts: Timeouts,
}

impl EbuildExecution {
    /// Creates an ebuild execution that owns the given `ipc` channel and direct `child`.
    pub fn new(ipc: IpcHandler, child: Child) -> Result<Self, PhaseExecutionError> {
        let pid = child
            .id()
            .context("spawned process has no PID")
            .map_err(PhaseExecutionError::Lifecycle)?;
        let process_group = Pid::from_raw(
            i32::try_from(pid)
                .context("ebuild PID does not fit in i32")
                .map_err(PhaseExecutionError::Lifecycle)?,
        );
        Ok(Self {
            ipc: Some(ipc),
            child,
            process_group,
            finalized: false,
            timeouts: Timeouts::default(),
        })
    }

    /// Runs the execution with the given IPC `handler`.
    ///
    /// Data is returned only when the child exits successfully.
    /// A non-zero exit is reported as [`PhaseExecutionError::NonZeroExit`].
    pub async fn run<T>(
        mut self,
        handler: impl AsyncFnOnce(&mut IpcHandler) -> Result<T, PhaseExecutionError>,
    ) -> Result<T, PhaseExecutionError> {
        let mut ipc = self
            .ipc
            .take()
            .expect("BUG: execution IPC is closed before run");

        match handler(&mut ipc).await {
            Ok(data) => {
                drop(ipc);
                let status = self.finalize().await?;
                match status.success() {
                    true => Ok(data),
                    false => Err(PhaseExecutionError::NonZeroExit(status)),
                }
            }
            // In case of a DIE, we still want to wait for the child to exit and close the
            // IPC channel afterwards
            Err(err @ PhaseExecutionError::Die(_)) => {
                let cleanup = self.finalize().await;
                drop(ipc);
                if let Err(cleanup) = cleanup {
                    warn!("unable to clean up failed ebuild execution: {cleanup:#}");
                }
                Err(err)
            }
            Err(err) => {
                drop(ipc);
                if let Err(cleanup) = self.finalize().await {
                    warn!("unable to clean up aborted ebuild execution: {cleanup:#}");
                }
                Err(err)
            }
        }
    }

    /// Finalizes the process, escalating to SIGTERM and SIGKILL as needed.
    async fn finalize(&mut self) -> Result<ExitStatus, PhaseExecutionError> {
        let status = if let Ok(status) = time::timeout(NATURAL_EXIT_PERIOD, self.child.wait()).await
        {
            status
                .context("unable to wait for process")
                .map_err(PhaseExecutionError::Lifecycle)?
        } else {
            match kill(self.process_group, Signal::SIGTERM) {
                Ok(()) | Err(Errno::ESRCH) => (),
                Err(err) => {
                    return Err(PhaseExecutionError::Lifecycle(anyhow!(
                        "unable to signal process: {err}"
                    )));
                }
            }
            let Some(status) = self
                .wait_for(self.timeouts.sigterm_grace, WaitUntil::ChildExited)
                .await?
            else {
                return self.kill_group_and_wait().await;
            };
            status
        };

        if process_group_exists(self.process_group)? {
            self.kill_group_and_wait().await
        } else {
            self.finalized = true;
            Ok(status)
        }
    }

    /// Kills the process group with SIGKILL and waits for it to exit.
    async fn kill_group_and_wait(&mut self) -> Result<ExitStatus, PhaseExecutionError> {
        match killpg(self.process_group, Signal::SIGKILL) {
            Ok(()) | Err(Errno::ESRCH) => (),
            Err(err) => {
                return Err(PhaseExecutionError::Lifecycle(anyhow!(
                    "unable to signal ebuild process group: {err}"
                )));
            }
        };

        let Some(status) = self
            .wait_for(self.timeouts.sigkill_wait, WaitUntil::GroupGone)
            .await?
        else {
            return Err(PhaseExecutionError::Lifecycle(anyhow!(
                "process group did not exit after SIGKILL"
            )));
        };
        self.finalized = true;
        Ok(status)
    }

    /// Waits until [`WaitUntil`] holds or `timeout` passes.
    ///
    /// Returns the child's status once the condition holds,
    /// or `None` on timeout.
    async fn wait_for(
        &mut self,
        timeout: Duration,
        until: WaitUntil,
    ) -> Result<Option<ExitStatus>, PhaseExecutionError> {
        let deadline = Instant::now() + timeout;
        loop {
            if Instant::now() >= deadline {
                return Ok(None);
            }

            let status = self
                .child
                .try_wait()
                .context("unable to inspect process")
                .map_err(PhaseExecutionError::Lifecycle)?;

            if let Some(status) = status {
                let group_exists = process_group_exists(self.process_group)?;
                match until {
                    WaitUntil::ChildExited => return Ok(Some(status)),
                    WaitUntil::GroupGone if !group_exists => return Ok(Some(status)),
                    _ => (),
                };
            }

            time::sleep(self.timeouts.poll).await;
        }
    }
}

impl Drop for EbuildExecution {
    fn drop(&mut self) {
        drop(self.ipc.take());
        if self.finalized {
            return;
        }
        // We cannot use `kill_group_and_wait` here due to async,
        // so we just send SIGKILL and hope for the best.
        warn!(
            "ebuild process group {} is still alive; sending SIGKILL...",
            self.process_group
        );
        let _ = killpg(self.process_group, Signal::SIGKILL);
    }
}

fn process_group_exists(pgroup: Pid) -> Result<bool, PhaseExecutionError> {
    match killpg(pgroup, None) {
        Ok(()) | Err(Errno::EPERM) => Ok(true),
        Err(Errno::ESRCH) => Ok(false),
        Err(err) => Err(err)
            .context("unable to inspect process group")
            .map_err(PhaseExecutionError::Lifecycle),
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

    fn spawn(script: &str) -> EbuildExecution {
        let args = vec!["-c".into(), script.into()];
        let (ipc, child) =
            IpcHandler::spawn(BASH_BINARY_PATH, &args, &FxHashMap::default()).unwrap();
        let mut execution = EbuildExecution::new(ipc, child).unwrap();
        execution.timeouts = TEST_TIMEOUTS;
        execution
    }

    async fn close_and_finalize(exec: &mut EbuildExecution) -> ExitStatus {
        drop(exec.ipc.take());
        exec.finalize().await.unwrap()
    }

    async fn wait_for_group(pg: Pid) {
        time::timeout(Duration::from_secs(1), async {
            while process_group_exists(pg).unwrap() {
                time::sleep(TEST_TIMEOUTS.poll).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_run_exit_status() {
        let exec = spawn("exit 0");
        assert!(exec.run(async |_| Ok(())).await.is_ok());

        let exec = spawn("exit 7");
        let err = exec.run(async |_| Ok(())).await.unwrap_err();
        assert!(
            matches!(err, PhaseExecutionError::NonZeroExit(status) if status.code() == Some(7))
        );
    }

    #[tokio::test]
    async fn test_run_die() {
        let exec = spawn("IFS= read -r <&${CHILD_READ_FD}");
        let pid = exec.process_group;

        let err = exec
            .run::<()>(async |_| Err(PhaseExecutionError::Die(String::new())))
            .await
            .unwrap_err();

        assert!(matches!(err, PhaseExecutionError::Die(_)));
        assert!(!process_group_exists(pid).unwrap());
    }

    #[tokio::test]
    async fn test_run_background_process_cleanup() {
        let exec = spawn("sleep 30 & exit 0");
        let started_at = Instant::now();

        let result = exec.run(async |_| Ok(42)).await;

        assert_eq!(result.unwrap(), 42);
        assert!(started_at.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn test_sigterm_exit() {
        let mut exec = spawn("trap 'exit 0' TERM; while :; do :; done");
        time::sleep(Duration::from_millis(20)).await;

        assert!(close_and_finalize(&mut exec).await.success());
    }

    #[tokio::test]
    async fn test_sigterm_escalation() {
        let mut exec = spawn("kill -STOP $$");
        time::sleep(Duration::from_millis(20)).await;
        let started_at = Instant::now();

        close_and_finalize(&mut exec).await;

        assert!(started_at.elapsed() >= TEST_TIMEOUTS.sigterm_grace);
    }

    #[tokio::test]
    async fn test_sigterm_orphan_cleanup() {
        let mut exec = spawn("trap 'exit 0' TERM; sleep 30 & while :; do :; done");
        exec.timeouts.sigterm_grace = Duration::from_secs(2);
        time::sleep(Duration::from_millis(20)).await;
        let started_at = Instant::now();

        close_and_finalize(&mut exec).await;

        assert!(started_at.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn test_process_group_isolation() {
        let mut first = spawn("kill -STOP $$");
        let mut second = spawn("while :; do :; done");
        time::sleep(Duration::from_millis(20)).await;

        close_and_finalize(&mut first).await;
        let second_is_running = second.child.try_wait().unwrap().is_none();
        close_and_finalize(&mut second).await;

        assert!(second_is_running);
    }

    #[tokio::test]
    async fn test_drop_cleanup() {
        let exec = spawn("sleep 30 & wait");
        let process_group = exec.process_group;

        drop(exec);
        wait_for_group(process_group).await;
    }
}
