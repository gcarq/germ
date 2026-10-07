pub mod cpv;
pub mod metadata;
pub mod names;
pub mod slot;
pub mod version;

use std::{fmt, hash};

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

use crate::atom::Atom;
use crate::repository::RepoName;

/// Provides a trait for package types used for common operations like comparison and atom
/// matching.
pub trait PackageView {
    fn cpv(&self) -> &CPV;
    fn repo(&self) -> &RepoName;
    fn metadata(&self) -> &PackageMetadata;

    /// Returns the category of the package.
    fn category(&self) -> &CatName {
        self.cpv().category()
    }

    /// Returns the name of the package.
    fn package(&self) -> &PkgName {
        self.cpv().package()
    }

    /// Returns the package version.
    fn version(&self) -> &PackageVersion {
        self.cpv().version()
    }

    /// Returns the package slot.
    fn slot(&self) -> &PackageSlot {
        self.metadata().slot()
    }

    /// Returns the qualified name of the package in the format `category/name`.
    fn qualified_name(&self) -> String {
        self.cpv().qualified_name()
    }

    /// Checks if self has the same category, name and primary slot as `other`.
    fn matches_package_slot<P: PackageView>(&self, other: &P) -> bool {
        self.category() == other.category()
            && self.package() == other.package()
            && self.slot().slot() == other.slot().slot()
    }

    /// Checks if the given [`Atom`] matches.
    fn matches_atom(&self, atom: &Atom) -> bool {
        if let Some(repo) = atom.repo()
            && repo != self.repo()
        {
            return false;
        }
        // TODO: evaluate `:=` and `:SLOT=` against the slot/sub-slot saved from
        // `DEPEND` match when resolving runtime dependencies.
        if let Some(slot) = atom.slot()
            && !slot.matches(self.metadata().slot())
        {
            return false;
        }
        atom.matches(self.cpv())
    }
}

/// Represents a repository package version together with metadata resolved from an available
/// repository's tree.
#[derive(Clone, Debug)]
pub struct Package {
    cpv: CPV,
    repo: RepoName,
    metadata: PackageMetadata,
}

impl Package {
    /// Creates a new [`Package`] from the given `cpv`, `repo` and `metadata`.
    pub const fn new(cpv: CPV, repo: RepoName, metadata: PackageMetadata) -> Self {
        Self {
            cpv,
            repo,
            metadata,
        }
    }
}

impl PackageView for Package {
    fn cpv(&self) -> &CPV {
        &self.cpv
    }

    fn repo(&self) -> &RepoName {
        &self.repo
    }

    fn metadata(&self) -> &PackageMetadata {
        &self.metadata
    }
}

impl Eq for Package {}

impl PartialEq for Package {
    fn eq(&self, other: &Self) -> bool {
        self.cpv == other.cpv && self.repo == other.repo
    }
}

impl hash::Hash for Package {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.cpv.hash(state);
        self.repo.hash(state);
    }
}

impl fmt::Display for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.cpv, self.repo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{assert_package_view_matches_atoms, pkg};

    #[test]
    fn test_package_display() {
        let pkg = pkg("dev-lang", "rust", "1.98.1", &[]);
        assert_eq!(pkg.to_string(), "dev-lang/rust-1.98.1::gentoo");
    }

    #[test]
    fn test_package_matches_atoms() {
        let pkg = pkg("sys-devel", "gcc", "15.2.1_p20251122-r1", &[("SLOT", "15")]);
        assert_package_view_matches_atoms(&pkg);
        assert_eq!(pkg.qualified_name(), "sys-devel/gcc");
    }
}
