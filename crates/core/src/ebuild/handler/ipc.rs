use super::protocol::{EBUILD_MESSAGE_DELIMITER, FUNCTION_REPLY_DELIMITER};
use crate::types::FxHashMap;
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, FdFlag, OFlag, fcntl};
use nix::unistd::pipe2;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::mem::ManuallyDrop;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process;
use thiserror::Error;

/// Errors caused by ebuild IPC setup or I/O.
#[derive(Error, Debug)]
pub enum IpcError {
    #[error("IPC pipe error")]
    Pipe(#[from] Errno),

    #[error("IPC I/O error")]
    Io(#[from] std::io::Error),
}

/// Handler for IPC communication via pipes with a child process.
/// When the `IpcHandler` is dropped, the write end of the pipe is flushed to ensure all data is
/// sent to the child process before closing.
///
/// **Example message from the ebuild process:**
///
/// `FN\0__resolve_eclass\0toolchain-funcs\4`
pub struct IpcHandler {
    reader: BufReader<File>,
    writer: File,
    buffer: Vec<u8>,
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
            .spawn()?;

        // SAFETY: We take ownership of the file descriptors and ensure with `ManuallyDrop` that
        // they are not closed when `OwnedFd` goes out of scope. They will only be dropped when
        // `IpcHandler` goes out of scope.
        let reader = unsafe { File::from_raw_fd(ManuallyDrop::new(parent_reader).as_raw_fd()) };
        let writer = unsafe { File::from_raw_fd(ManuallyDrop::new(parent_writer).as_raw_fd()) };

        Ok((
            Self {
                buffer: Vec::with_capacity(256),
                reader: BufReader::new(reader),
                writer,
            },
            child,
        ))
    }

    /// Sends encoded response bytes to the child process.
    /// The data must not contain [`FUNCTION_REPLY_DELIMITER`], which is added automatically.
    pub fn send(&mut self, bytes: &[u8]) -> Result<(), IpcError> {
        self.writer
            .write_all(bytes)
            .and_then(|()| self.writer.write_all(FUNCTION_REPLY_DELIMITER))
            .and_then(|()| self.writer.flush())?;
        Ok(())
    }

    /// Reads raw bytes from the child process until [`EBUILD_MESSAGE_DELIMITER`] is encountered.
    /// Returns `Ok(None)` if EOF is reached.
    pub fn recv_bytes(&mut self) -> Result<Option<&[u8]>, IpcError> {
        self.buffer.clear();
        let num_bytes = self
            .reader
            .read_until(EBUILD_MESSAGE_DELIMITER, &mut self.buffer)?;
        // We got EOF
        if num_bytes == 0 {
            return Ok(None);
        }
        // Get rid of the EOT character
        self.buffer.truncate(self.buffer.len() - 1);
        Ok(Some(&self.buffer))
    }
}

impl Drop for IpcHandler {
    fn drop(&mut self) {
        let _ = self.writer.flush();
    }
}

#[cfg(test)]
mod tests {
    use crate::consts::BASH_BINARY_PATH;

    use super::*;

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

    fn recv(ipc: &mut IpcHandler) -> String {
        String::from_utf8(ipc.recv_bytes().unwrap().unwrap().to_owned()).unwrap()
    }

    #[test]
    fn test_ipc_reply_state() {
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
            assert_eq!(recv(&mut ipc), request);
            ipc.send(reply.as_bytes()).unwrap();
            assert_eq!(recv(&mut ipc), result);
        }

        assert!(process.wait().unwrap().success());
    }

    #[test]
    fn test_ipc_handler_pipes() {
        let (mut ipc, mut process) = spawn_process(
            r#"
                IFS= read -r response <&${CHILD_READ_FD} || exit 1
                printf '%s\4' "${response}" >&${CHILD_WRITE_FD} || exit 1
                printf '%s\4' "${TEST_ENV}" >&${CHILD_WRITE_FD} || exit 1
            "#,
            [("TEST_ENV", "42")],
        );

        ipc.send(b"sync").unwrap();
        assert_eq!(recv(&mut ipc), "sync", "expected echoed line");
        assert_eq!(recv(&mut ipc), "42", "expected value from env variable");

        assert!(process.wait().unwrap().success());
    }

    #[test]
    fn test_fatal_die_exits_shell_after_ipc_reply() {
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

        recv(&mut ipc);
        ipc.send(b"DIE").unwrap();

        assert_eq!(process.wait().unwrap().code(), Some(1));
    }
}
