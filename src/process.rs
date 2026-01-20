use crate::consts::BASH_BINARY_PATH;
use anyhow::{Context, Result};
use std::process::{Child, Command, Stdio};

pub enum EbuildPhase {
    Metadata,
}

pub struct EbuildProcess {
    phase: EbuildPhase,
    process: Child,
}

impl EbuildProcess {
    pub fn new(phase: EbuildPhase) -> Result<Self> {
        let process = Command::new(BASH_BINARY_PATH)
            .arg("-c")
            .arg("exec \"$@\"")
            .stdout(Stdio::piped())
            .spawn()
            .with_context(|| "unable to process ebuild phase")?;

        Ok(Self { phase, process })
    }
}
