pub mod cache;
pub mod discovery;
mod error;
mod index;

pub use error::PackageResolutionError;
use germ_pms::Package;
pub use index::CpvIndex;

/// Alias for a package resolution operation
pub type PackageResult = Result<Package, PackageResolutionError>;
