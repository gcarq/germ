use anyhow::{Context, Result, anyhow};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::mem::ManuallyDrop;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;

/// Handler for IPC communication with a bash child process.
pub struct IpcHandler {
    writer: Option<File>,
    reader: BufReader<File>,
}

impl IpcHandler {
    /// Creates a new `Ipc` instance from the given `OwnedFd` descriptors.
    /// SAFETY: The caller must ensure that the provided file descriptors are valid
    /// and are not used elsewhere after this call.
    pub unsafe fn new(writer: ManuallyDrop<OwnedFd>, reader: ManuallyDrop<OwnedFd>) -> Self {
        let to_bash = unsafe { File::from_raw_fd(writer.as_raw_fd()) };
        let from_bash = unsafe { File::from_raw_fd(reader.as_raw_fd()) };
        Self {
            writer: Some(to_bash),
            reader: BufReader::new(from_bash),
        }
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
