use super::protocol::{EBUILD_MESSAGE_DELIMITER, FUNCTION_REPLY_DELIMITER};
use crate::types::FxHashMap;
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, FdFlag, OFlag, fcntl};
use nix::unistd::pipe2;
use std::io;
use std::os::fd::AsRawFd;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::pipe::{Receiver, Sender};
use tokio::process;

/// Errors caused by ebuild IPC setup or I/O.
#[derive(Error, Debug)]
pub enum IpcError {
    #[error("IPC pipe error")]
    Pipe(#[from] Errno),

    #[error("unable to spawn IPC process")]
    Spawn(#[source] io::Error),

    #[error("IPC I/O error")]
    Io(#[from] io::Error),

    #[error("incomplete IPC message")]
    IncompleteMessage,
}

/// Handler for IPC communication via pipes with a child process.
/// Each response is written completely by [`Self::send`] before it returns; dropping the handler
/// closes the pipe ends.
///
/// **Example message from the ebuild process:**
///
/// `FN\0__resolve_eclass\0toolchain-funcs\4`
pub struct IpcHandler {
    reader: BufReader<Receiver>,
    writer: Sender,
    recvbuf: Vec<u8>,
    sendbuf: Vec<u8>,
}

impl IpcHandler {
    /// Spawns a child process with the given `executable`, `args`, and `env`.
    ///
    /// The new process gets two pipes for communication with the parent process.
    /// The file descriptors are injected via ENV variables `CHILD_READ_FD` and `CHILD_WRITE_FD`.
    pub fn spawn(
        executable: &str,
        args: &[String],
        env: &FxHashMap<String, String>,
    ) -> Result<(Self, process::Child), IpcError> {
        // parent -> child pipes
        let (child_reader, parent_writer) = pipe2(OFlag::O_CLOEXEC)?;
        // child -> parent pipes
        let (parent_reader, child_writer) = pipe2(OFlag::O_CLOEXEC)?;

        // Clear O_CLOEXEC for fds we want to pass to the child process
        fcntl(&child_reader, FcntlArg::F_SETFD(FdFlag::empty()))?;
        fcntl(&child_writer, FcntlArg::F_SETFD(FdFlag::empty()))?;

        let child = process::Command::new(executable)
            .args(args)
            .env_clear()
            .env("CHILD_READ_FD", child_reader.as_raw_fd().to_string())
            .env("CHILD_WRITE_FD", child_writer.as_raw_fd().to_string())
            .envs(env)
            .process_group(0)
            .spawn()
            .map_err(IpcError::Spawn)?;

        Ok((
            Self {
                reader: BufReader::new(Receiver::from_owned_fd(parent_reader)?),
                writer: Sender::from_owned_fd(parent_writer)?,
                recvbuf: Vec::with_capacity(256),
                sendbuf: Vec::with_capacity(128),
            },
            child,
        ))
    }

    /// Sends encoded response bytes to the child process.
    /// The data must not contain [`FUNCTION_REPLY_DELIMITER`], which is added automatically.
    pub async fn send(&mut self, bytes: &[u8]) -> Result<(), IpcError> {
        self.sendbuf.clear();
        self.sendbuf.extend_from_slice(bytes);
        self.sendbuf.extend_from_slice(FUNCTION_REPLY_DELIMITER);
        self.writer.write_all(&self.sendbuf).await?;
        Ok(())
    }

    /// Reads raw bytes from the child process until [`EBUILD_MESSAGE_DELIMITER`] is encountered.
    /// Returns `Ok(None)` if EOF is reached.
    pub async fn recv_bytes(&mut self) -> Result<Option<&[u8]>, IpcError> {
        self.recvbuf.clear();
        let num_bytes = self
            .reader
            .read_until(EBUILD_MESSAGE_DELIMITER, &mut self.recvbuf)
            .await?;
        // We got EOF
        if num_bytes == 0 {
            return Ok(None);
        }
        if self.recvbuf.last() != Some(&EBUILD_MESSAGE_DELIMITER) {
            return Err(IpcError::IncompleteMessage);
        }

        // Get rid of the EOT character
        self.recvbuf.truncate(self.recvbuf.len() - 1);
        Ok(Some(&self.recvbuf))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::BASH_BINARY_PATH;

    fn spawn_process<const N: usize>(
        script: &str,
        env: [(&str, &str); N],
    ) -> (IpcHandler, process::Child) {
        let args = vec!["-c".into(), script.into()];
        let env = env
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        IpcHandler::spawn(BASH_BINARY_PATH, &args, &env).unwrap()
    }

    async fn recv(ipc: &mut IpcHandler) -> String {
        String::from_utf8(ipc.recv_bytes().await.unwrap().unwrap().to_owned()).unwrap()
    }

    #[tokio::test]
    async fn test_ipc_reply_state() {
        let (mut ipc, mut process) = spawn_process(
            r#"
                source "${INTERNALS_PATH}" || exit 1
                capture_reply() {
                    local __ipc_reply=stale
                    __ipc_call payload >/dev/null || exit 1
                    printf 'RESULT\0payload\0%s\4' "${__ipc_reply}" >&${CHILD_WRITE_FD} || exit 1

                    __ipc_reply=stale
                    __ipc_call no-payload >/dev/null || exit 1
                    printf 'RESULT\0no-payload\0%s\4' "${__ipc_reply}" >&${CHILD_WRITE_FD} || exit 1

                    __ipc_reply=stale
                    if __ipc_call failure >/dev/null; then
                        exit 1
                    fi
                    printf 'RESULT\0failure\0%s\4' "${__ipc_reply}" >&${CHILD_WRITE_FD} || exit 1
                }
                capture_reply
            "#,
            [(
                "INTERNALS_PATH",
                concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/internals.sh"),
            )],
        );

        for (request, reply, result) in [
            ("FN\0payload", "OK value", "RESULT\0payload\0value"),
            ("FN\0no-payload", "OK", "RESULT\0no-payload\0"),
            ("FN\0failure", "ERR", "RESULT\0failure\0"),
        ] {
            assert_eq!(recv(&mut ipc).await, request);
            ipc.send(reply.as_bytes()).await.unwrap();
            assert_eq!(recv(&mut ipc).await, result);
        }

        assert!(process.wait().await.unwrap().success());
    }

    #[tokio::test]
    async fn test_ipc_handler_pipes() {
        let (mut ipc, mut process) = spawn_process(
            r#"
                IFS= read -r response <&${CHILD_READ_FD} || exit 1
                printf '%s\4' "${response}" >&${CHILD_WRITE_FD} || exit 1
                printf '%s\4' "${TEST_ENV}" >&${CHILD_WRITE_FD} || exit 1
            "#,
            [("TEST_ENV", "42")],
        );

        ipc.send(b"sync").await.unwrap();
        assert_eq!(recv(&mut ipc).await, "sync", "expected echoed line");
        assert_eq!(
            recv(&mut ipc).await,
            "42",
            "expected value from env variable"
        );

        assert!(process.wait().await.unwrap().success());
    }

    #[tokio::test]
    async fn test_incomplete_message_is_rejected() {
        let (mut ipc, mut process) = spawn_process("printf 'partial' >&${CHILD_WRITE_FD}", []);

        let result = ipc.recv_bytes().await;
        assert!(matches!(result, Err(IpcError::IncompleteMessage)));
        process.wait().await.unwrap();
    }

    #[tokio::test]
    async fn test_fatal_die_exits_shell_after_ipc_reply() {
        let (mut ipc, mut process) = spawn_process(
            r#"
                cd "${PROJECT_ROOT}" || exit 2
                source "./bin/functions.sh" || exit 2
                die fatal
                exit 0
            "#,
            [(
                "PROJECT_ROOT",
                concat!(env!("CARGO_MANIFEST_DIR"), "/../.."),
            )],
        );

        recv(&mut ipc).await;
        ipc.send(b"DIE").await.unwrap();

        assert_eq!(process.wait().await.unwrap().code(), Some(1));
    }
}
