use anyhow::anyhow;
use thiserror::Error;

use crate::{
    ebuild::{
        EbuildError,
        handler::error::{MetadataGenerationError, PhaseExecutionError},
    },
    repository::RepositoryError,
};

/// Defines failures while resolving packages from repositories.
#[derive(Debug, Error)]
#[error("{cpv}: {source}")]
pub struct PackageResolutionError {
    pub cpv: String,
    #[source]
    pub source: MetadataGenerationError,
}

impl PackageResolutionError {
    pub fn new(cpv: impl Into<String>, source: MetadataGenerationError) -> Self {
        Self {
            cpv: cpv.into(),
            source,
        }
    }

    /// Promotes a [`PackageResolutionError`] to a [`RepositoryError`], if applicable.
    pub fn promote(self) -> Result<Self, RepositoryError> {
        match self.source {
            error @ MetadataGenerationError::Ebuild(EbuildError::Io { .. }) => {
                Err(RepositoryError::Data(anyhow!(error)))
            }
            error @ (MetadataGenerationError::Internal(_)
            | MetadataGenerationError::Execution(
                PhaseExecutionError::Protocol(_)
                | PhaseExecutionError::Ipc(_)
                | PhaseExecutionError::Invariant(_),
            )) => Err(RepositoryError::Internal(anyhow!(error))),
            _ => Ok(self),
        }
    }
}
