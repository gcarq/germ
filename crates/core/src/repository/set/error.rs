use super::super::tree::RepositoryError;
use thiserror::Error;

/// Defines failures for working with a repository set.
#[derive(Debug, Error)]
pub enum RepoSetError {
    #[error("repos.conf configuration failure")]
    Configuration(#[source] anyhow::Error),

    #[error("repository inheritance cycle detected, involving '{0}'")]
    Cycle(String),

    /// A repository operation failed while processing the set.
    #[error("operation failed for repository '{repository}'")]
    Repository {
        repository: String,
        #[source]
        source: RepositoryError,
    },

    #[error("internal reposet error")]
    Internal(#[source] anyhow::Error),
}

impl RepoSetError {
    pub fn repo_failure(repository: &str, source: RepositoryError) -> Self {
        Self::Repository {
            repository: repository.to_owned(),
            source,
        }
    }
}
