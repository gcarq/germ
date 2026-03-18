use crate::types::FxHashMap;
use anyhow::{Context, Result};
use nix::fcntl::{FcntlArg, FdFlag, OFlag, fcntl};
use nix::unistd::pipe2;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::mem::ManuallyDrop;
use std::os::fd::{AsRawFd, FromRawFd};
use std::process;
use std::process::Command;

/// Trait for messages sent from the parent process to the child process.
pub trait ParentToChildMsg {
    fn into_bytes(self) -> Vec<u8>;
}

/// Trait for messages sent from the child process to the parent process.
pub trait ChildToParentMsg: Sized {
    fn from_bytes(bytes: &[u8]) -> Result<Self>;
}

/// Handler for IPC communication via pipes with a child process.
/// When the `IpcHandler` is dropped, the write end of the pipe is flushed to ensure all data is
/// sent to the child process before closing.
///
/// **Example message from child to parent process:**
/// ```
/// FN\0__resolve_eclass\0toolchain-funcs\4
/// ```
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
    ) -> Result<(Self, process::Child)> {
        // parent -> child pipes
        let (child_reader, parent_writer) = pipe2(OFlag::O_CLOEXEC).context("pipe failed")?;
        // child -> parent pipes
        let (parent_reader, child_writer) = pipe2(OFlag::O_CLOEXEC).context("pipe failed")?;

        // Clear O_CLOEXEC for fds we want to pass to the child process
        fcntl(&child_reader, FcntlArg::F_SETFD(FdFlag::empty()))?;
        fcntl(&child_writer, FcntlArg::F_SETFD(FdFlag::empty()))?;

        let child = Command::new(executable)
            .args(args)
            .env_clear()
            .env("CHILD_READ_FD", child_reader.as_raw_fd().to_string())
            .env("CHILD_WRITE_FD", child_writer.as_raw_fd().to_string())
            .envs(env)
            .spawn()
            .context("spawn failed")?;

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

    /// Sends the given [`ParentToChildMsg`] to the child process.
    /// The data sent must not contain a newline character; one will be added automatically.
    pub fn send<T>(&mut self, msg: T) -> Result<()>
    where
        T: ParentToChildMsg,
    {
        self.writer
            .write_all(&msg.into_bytes())
            .and_then(|()| self.writer.write_all(b"\n"))
            .and_then(|()| self.writer.flush())
            .with_context(|| "failed to write to pipe")
    }

    /// Reads raw bytes from the child process until `\4` is encountered.
    /// Returns [`ChildToParentMsg`] or `Ok(None)` if EOF is reached.
    pub fn recv<T>(&mut self) -> Result<Option<T>>
    where
        T: ChildToParentMsg,
    {
        self.buffer.clear();
        let num_bytes = self
            .reader
            .read_until(4, &mut self.buffer)
            .with_context(|| "failed to read from child process")?;
        // We got EOF
        if num_bytes == 0 {
            return Ok(None);
        }
        // Get rid of the EOT character
        self.buffer.truncate(self.buffer.len() - 1);
        Ok(Some(T::from_bytes(&self.buffer)?))
    }
}

impl Drop for IpcHandler {
    fn drop(&mut self) {
        let _ = self.writer.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_ipc_handler_pipes() {
        let args = vec![
            "-c".into(),
            r#"
                IFS= read -r response <&${CHILD_READ_FD} || exit 1
                printf '%s\4' "${response}" >&${CHILD_WRITE_FD} || exit 1
                printf '%s\4' "${TEST_ENV}" >&${CHILD_WRITE_FD} || exit 1
                sleep 30
            "#
            .into(),
        ];
        let env: FxHashMap<String, String> =
            FxHashMap::from_iter([("TEST_ENV".into(), "42".into())]);

        let (mut ipc, mut process) = IpcHandler::spawn("/usr/bin/bash", &args, &env).unwrap();

        ipc.send(ParentTestMsg("sync".into())).unwrap();
        let resp = ipc.recv::<ChildTestMsg>().unwrap().unwrap();
        assert_eq!(resp.0, "sync", "expected echoed line");

        let resp = ipc.recv::<ChildTestMsg>().unwrap().unwrap();
        assert_eq!(resp.0, "42", "expected value from env variable");

        process.kill().unwrap();
    }
}
