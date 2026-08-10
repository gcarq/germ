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
        match &self.source {
            MetadataGenerationError::Ebuild(EbuildError::Io { .. }) => {
                Err(RepositoryError::Data(anyhow!(self)))
            }
            MetadataGenerationError::Internal(_)
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
        let RepositoryError::Data(source) = data.promote().unwrap_err() else {
            panic!();
        };
        assert_eq!(
            source.to_string(),
            "app-misc/foo-1: unable to read ebuild foo-1.ebuild"
        );

        let internal = PackageResolutionError::new(
            "app-misc/foo-1",
            MetadataGenerationError::Internal(anyhow::anyhow!("test")),
        );
        let RepositoryError::Internal(source) = internal.promote().unwrap_err() else {
            panic!();
        };
        assert_eq!(
            source.to_string(),
            "app-misc/foo-1: internal error while preparing ebuild execution"
        );
    }
}
