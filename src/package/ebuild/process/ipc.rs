use anyhow::{Context, Result, anyhow};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::mem::ManuallyDrop;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

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

    /// Sends the given `line` of text to the bash process.
    /// The line should not include a newline character; one will be added automatically.
    pub fn send(&mut self, line: &str) -> Result<()> {
        let to_bash = match &mut self.writer {
            Some(writer) => writer,
            None => return Err(anyhow!("bash writer is closed")),
        };
        to_bash
            .write_all(line.as_bytes())
            .and_then(|_| to_bash.write_all(b"\n"))
            .and_then(|_| to_bash.flush())
            .with_context(|| "failed to write to bash")
    }

    /// Receives a line of text from the child process.
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
}

impl Drop for IpcHandler {
    fn drop(&mut self) {
        if let Some(mut writer) = self.writer.take() {
            let _ = writer.flush();
        }
    }
}
