use anyhow::{Context, Result, anyhow};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::spawn::PosixSpawnFileActions;
use nix::unistd::{Pid, pipe};
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::mem::ManuallyDrop;
use std::os::fd::{AsFd, AsRawFd, FromRawFd};
use std::time::Duration;

/// Handler for IPC communication via pipes with a child process.
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

    /// Sends the given `Resp` of text to the bash process.
    /// The data sent should not include a newline character; one will be added automatically.
    pub fn send<Resp: fmt::Display>(&mut self, response: &Resp) -> Result<()> {
        let to_bash = match &mut self.writer {
            Some(writer) => writer,
            None => return Err(anyhow!("bash writer is closed")),
        };
        to_bash
            .write_all(response.to_string().as_bytes())
            .and_then(|_| to_bash.write_all(b"\n"))
            .and_then(|_| to_bash.flush())
            .with_context(|| "failed to write to bash")
    }

    /// Receives a line of text from the child process without the ending newline.
    /// Returns `Ok(None)` if EOF is reached.
    pub fn recv(&mut self) -> Result<Option<String>> {
        let mut buf = String::new();
        match self.reader.read_line(&mut buf) {
            Ok(0) => Ok(None), // EOF
            Ok(_) => {
                if buf.ends_with('\n') {
                    buf.pop();
                }
                Ok(Some(buf))
            }
            Err(e) => Err(anyhow!(e)).with_context(|| "failed to read from child process"),
        }
    }

    /// Waits for data to be available for reading from the child process.
    /// Times out after 5 seconds.
    /// Returns `Ok(true)` if data is available, `Ok(false)` if timed out.
    pub fn poll(&mut self) -> Result<bool> {
        let timeout = PollTimeout::try_from(Duration::from_secs(5))?;
        let fd = PollFd::new(self.reader.get_ref().as_fd(), PollFlags::POLLIN);
        match poll(&mut [fd], timeout)? {
            0 => Ok(false),
            _ => Ok(true),
        }
    }
}

impl Drop for IpcHandler {
    fn drop(&mut self) {
        if let Some(mut writer) = self.writer.take() {
            let _ = writer.flush();
        }
    }
}
