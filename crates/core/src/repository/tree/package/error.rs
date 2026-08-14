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
#[error("resolving metadata for {cpv} failed")]
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
        match &self.source {
            MetadataGenerationError::Ebuild(EbuildError::Io { .. }) => {
                Err(RepositoryError::Data(anyhow!(self)))
            }
            MetadataGenerationError::Ebuild(EbuildError::Internal(_))
            | MetadataGenerationError::Internal(_)
            | MetadataGenerationError::Execution(
                PhaseExecutionError::Protocol(_)
                | PhaseExecutionError::Ipc(_)
                | PhaseExecutionError::Invariant(_),
            ) => Err(RepositoryError::Internal(anyhow!(self))),
            _ => Ok(self),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{io, path::PathBuf};

    use super::*;

    #[test]
    fn test_promote() {
        let data = PackageResolutionError::new(
            "app-misc/foo-1",
            MetadataGenerationError::Ebuild(EbuildError::Io {
                path: PathBuf::from("foo-1.ebuild"),
                source: io::Error::from(io::ErrorKind::NotFound),
            }),
        );
        let RepositoryError::Data(_) = data.promote().unwrap_err() else {
            panic!();
        };

        let internal = PackageResolutionError::new(
            "app-misc/foo-1",
            MetadataGenerationError::Internal(anyhow::anyhow!("test")),
        );
        let RepositoryError::Internal(_) = internal.promote().unwrap_err() else {
            panic!();
        };

        let lifecycle = PackageResolutionError::new(
            "app-misc/foo-1",
            MetadataGenerationError::Execution(PhaseExecutionError::Lifecycle(anyhow::anyhow!(
                "test"
            ))),
        );
        assert!(lifecycle.promote().is_ok());
    }
}
