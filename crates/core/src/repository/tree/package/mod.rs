pub mod cache;
mod discovery;
mod error;
mod index;

pub(super) use discovery::resolve_cpv_from_category;
pub use error::PackageResolutionError;
pub(crate) use index::CPVIndex;
