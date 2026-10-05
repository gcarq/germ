pub mod cpv;
pub mod metadata;
pub mod names;
pub mod slot;
pub mod version;

pub use cpv::CPV;
pub use metadata::{
    DependencyField, MetaVar, PackageMetadata, PackageMetadataError, RawPackageMetadata,
};
pub use names::{CatName, PkgName};
pub use slot::{PackageSlot, SlotName};
pub use version::{
    NumberComponent, NumericComponent, PackageRevision, PackageVersion, VersionNumber,
    VersionSuffix, VersionSuffixes,
};
