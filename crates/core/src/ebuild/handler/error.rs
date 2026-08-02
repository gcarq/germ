use std::process::ExitStatus;
use thiserror::Error;

/// Errors returned while executing an ebuild phase.
#[derive(Error, Debug)]
pub enum ExecutionError {
    #[error("ebuild died: {0}")]
    Die(String),

    #[error("ebuild process exited with non-zero status: {0}")]
    NonZeroExit(ExitStatus),

    #[error("internal execution error")]
    Internal(#[from] anyhow::Error),
}
