use std::process::ExitStatus;

use thiserror::Error;

use crate::eapi::Eapi;
use crate::ebuild::EbuildError;
use crate::package::metadata::PackageMetadataError;

use super::protocol::FuncType;

pub use super::ipc::IpcError;
pub use super::protocol::ProtocolError;

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

    #[error("eclass '{name}' not found in {repository} or its masters")]
    EclassNotFound { name: String, repository: String },

    #[error("function '{func}' is unsupported for EAPI '{eapi}'")]
    Unsupported { func: FuncType, eapi: Eapi },
}

/// Errors returned during ebuild execution.
#[derive(Error, Debug)]
pub enum PhaseExecutionError {
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

    #[error("ebuild process failed")]
    Lifecycle(#[source] anyhow::Error),

    #[error("ebuild process failed")]
    Invariant(#[source] anyhow::Error),
}

/// Errors returned when generating package metadata for a [`CPV`].
#[derive(Debug, Error)]
pub enum MetadataGenerationError {
    #[error(transparent)]
    Ebuild(#[from] EbuildError),

    #[error("internal error while preparing ebuild execution")]
    Internal(#[from] anyhow::Error),

    #[error(transparent)]
    Execution(#[from] PhaseExecutionError),

    #[error(transparent)]
    Metadata(#[from] PackageMetadataError),
}
