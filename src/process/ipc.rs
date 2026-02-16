use anyhow::{Context, Result, anyhow};
use nix::spawn::PosixSpawnFileActions;
use nix::unistd::{Pid, pipe};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::mem::ManuallyDrop;
use std::os::fd::{AsRawFd, FromRawFd};

/// Trait for messages sent from the parent process to the child process.
pub trait ParentMessage {
    fn into_bytes(self) -> Vec<u8>;
}

/// Trait for messages sent from the child process to the parent process.
pub trait ChildMessage: Sized {
    fn from_bytes(bytes: Vec<u8>) -> Result<Self>;
}

/// Handler for IPC communication via pipes with a child process.
/// When the `IpcHandler` is dropped, the write end of the pipe is flushed to ensure all data is
/// sent to the child process before closing.
///
/// The child process is expected to use file descriptors 10 and 11 for reading and writing,
/// respectively. The child process must ensure that a message doesn't contain any
/// newline characters, as they are used to delimit messages.
///
/// **Example message from child to parent process:**
/// ```
/// FN __resolve_eclass toolchain-funcs\n
/// ```
///
pub struct IpcHandler {
    reader: BufReader<File>,
    writer: Option<File>,
}

impl IpcHandler {
    /// Creates a new `Ipc` instance. The provided closure is responsible for spawning the child
    /// process with the given `PosixSpawnFileActions`. The closure must also return the PID of
    /// the spawned child process.
    ///
    /// # Example
    ///
    /// ```
    /// let attrs = PosixSpawnAttr::init()?;
    /// let (ipc, pid) = IpcHandler::new(|actions| {
    ///     posix_spawn(binary, &actions, &attrs, &args, &env)
    /// })
    /// ```
    pub fn new<F>(func: F) -> Result<(Self, Pid)>
    where
        F: FnOnce(PosixSpawnFileActions) -> Result<Pid>,
    {
        // IPC pipe for parent -> child
        let (child_reader, parent_writer) = pipe().with_context(|| "unable to create pipe")?;
        // IPC pipe for child -> parent
        let (parent_reader, child_writer) = pipe().with_context(|| "unable to create pipe")?;

        let mut actions =
            PosixSpawnFileActions::init().with_context(|| "unable to init posix_spawn actions")?;
        actions.add_dup2(child_reader.as_raw_fd(), 10)?;
        actions.add_dup2(child_writer.as_raw_fd(), 11)?;
        actions.add_close(parent_reader.as_raw_fd())?;
        actions.add_close(parent_writer.as_raw_fd())?;

        // SAFETY: We take ownership of the file descriptors and ensure with `ManuallyDrop` that
        // they are not closed when `OwnedFd` goes out of scope. They will only be dropped when
        // `IpcHandler` goes out of scope.
        let reader = unsafe { File::from_raw_fd(ManuallyDrop::new(parent_reader).as_raw_fd()) };
        let writer = unsafe { File::from_raw_fd(ManuallyDrop::new(parent_writer).as_raw_fd()) };

        // Create IPC and spawn child process via the given closure.
        let instance = Self {
            reader: BufReader::new(reader),
            writer: Some(writer),
        };
        let pid = func(actions)?;

        // The child pipe ends are now owned by the child process,
        // so we can safely drop them in the parent.
        drop(child_reader);
        drop(child_writer);

        Ok((instance, pid))
    }

    /// Sends the given [`ParentMessage`] to the child process.
    /// The data sent must not contain a newline character; one will be added automatically.
    pub fn send<T: ParentMessage>(&mut self, msg: T) -> Result<()> {
        let writer = match &mut self.writer {
            Some(writer) => writer,
            None => return Err(anyhow!("bash writer is closed")),
        };
        writer
            .write_all(&msg.into_bytes())
            .and_then(|_| writer.write_all(b"\n"))
            .and_then(|_| writer.flush())
            .with_context(|| "failed to write to bash")
    }

    /// Reads raw bytes from the child process until `\n` is encountered.
    /// Returns [`ChildMessage`] or `Ok(None)` if EOF is reached.
    pub fn recv<T: ChildMessage>(&mut self) -> Result<Option<T>> {
        let mut buf = Vec::new();
        let num_bytes = (self.reader.read_until(b'\n', &mut buf))
            .with_context(|| "failed to read from child process")?;
        // We got EOF
        if num_bytes == 0 {
            return Ok(None);
        };
        buf.pop_if(|b| *b == b'\n');
        Ok(Some(T::from_bytes(buf)?))
    }
}

impl Drop for IpcHandler {
    fn drop(&mut self) {
        if let Some(mut writer) = self.writer.take() {
            let _ = writer.flush();
        }
    }
}
