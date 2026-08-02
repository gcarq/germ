pub use super::ipc::IpcError;
use super::protocol::FuncType;
pub use super::protocol::ProtocolError;
use crate::eapi::Eapi;
use std::process::ExitStatus;
use thiserror::Error;

/// Errors returned while handling an ebuild function call.
#[derive(Error, Debug)]
pub enum FuncCallError {
    #[error("invalid arguments for function '{func}': {args:?}")]
    InvalidArgs {
        func: FuncType,
        args: Vec<String>,
        #[source]
        source: anyhow::Error,
    },

    #[error("function '{func}' is unsupported for EAPI '{eapi}'")]
    Unsupported { func: FuncType, eapi: Eapi },
}

/// Errors returned during ebuild execution.
#[derive(Error, Debug)]
pub enum ExecutionError {
    #[error(transparent)]
    FuncCall(#[from] FuncCallError),

    #[error(transparent)]
    Protocol(#[from] ProtocolError),

    #[error(transparent)]
    Ipc(#[from] IpcError),

    #[error("ebuild died: {0}")]
    Die(String),

    #[error("ebuild process exited with non-zero status: {0}")]
    NonZeroExit(ExitStatus),

    #[error("internal execution error")]
    Internal(#[from] anyhow::Error),
}
