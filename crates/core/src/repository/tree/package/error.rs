use crate::ebuild::EbuildError;
use crate::ebuild::handler::error::PhaseExecutionError;
use crate::package::cpv::{CPV, MetadataGenerationError};
use thiserror::Error;

/// Defines failures while resolving packages from repositories.
#[derive(Debug, Error)]
pub enum PackageResolutionError {
    #[error("{cpv}: {source}")]
    Metadata {
        cpv: String,
        #[source]
        source: MetadataGenerationError,
    },

    #[error("{cpv}: internal package resolution error")]
    Internal {
        cpv: String,
        #[source]
        source: anyhow::Error,
    },
}

impl PackageResolutionError {
    pub(crate) fn from_metadata(cpv: &CPV, error: MetadataGenerationError) -> Self {
        let cpv = cpv.fqn().to_owned();
        match error {
            error @ (MetadataGenerationError::Ebuild(EbuildError::Io { .. })
            | MetadataGenerationError::Internal(_)
            | MetadataGenerationError::Execution(
                PhaseExecutionError::Protocol(_)
                | PhaseExecutionError::Ipc(_)
                | PhaseExecutionError::Internal(_),
            )) => Self::Internal {
                cpv,
                source: error.into(),
            },
            source => Self::Metadata { cpv, source },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ebuild::handler::error::ProtocolError;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    #[test]
    fn test_package_resolution_error_conversion() {
        let metadata = MetadataGenerationError::Execution(PhaseExecutionError::NonZeroExit(
            ExitStatus::from_raw(1),
        ));

        let internal = MetadataGenerationError::Execution(PhaseExecutionError::Protocol(
            ProtocolError::MissingFunction,
        ));

        assert!(matches!(
            PackageResolutionError::from_metadata(&CPV::default(), metadata),
            PackageResolutionError::Metadata { .. }
        ));

        assert!(matches!(
            PackageResolutionError::from_metadata(&CPV::default(), internal),
            PackageResolutionError::Internal { .. }
        ));
    }
}
