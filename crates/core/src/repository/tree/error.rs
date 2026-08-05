use super::layout::LayoutError;
use super::profiles::ProfileError;
use thiserror::Error;

/// Defines failures while loading or accessing an available repository tree.
#[derive(Debug, Error)]
pub enum RepositoryError {
    /// Data cannot be loaded, it might be either missing or malformed.
    #[error("repository data failure")]
    Data(#[source] anyhow::Error),

    #[error("unable to process layout.conf")]
    Layout(#[from] LayoutError),

    #[error("unable to process profile directory")]
    Profile(#[from] ProfileError),

    #[error("internal repository error")]
    Internal(#[source] anyhow::Error),
}
