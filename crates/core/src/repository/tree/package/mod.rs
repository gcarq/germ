pub mod cache;
pub mod discovery;
mod error;
mod index;

pub use error::PackageResolutionError;
pub use index::CPVIndex;

use crate::package::Package;

/// Alias for a package resolution operation
pub type PackageResult = Result<Package, PackageResolutionError>;
