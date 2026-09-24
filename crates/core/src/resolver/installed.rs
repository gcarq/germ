use std::future::Future;

use crate::atom::Atom;
use crate::vdb::{Vdb, package::InstalledPackage};

/// Defines a common trait to access installed packages.
pub trait VdbPackages: Send {
    /// Returns [`InstalledPackage`]s that match the given `atom`.
    fn find_by_atom(
        &mut self,
        atom: &Atom,
    ) -> impl Future<Output = anyhow::Result<Vec<InstalledPackage>>> + Send;
}

impl VdbPackages for Vdb {
    async fn find_by_atom(&mut self, atom: &Atom) -> anyhow::Result<Vec<InstalledPackage>> {
        Ok(self.find_by_atom(atom)?.cloned().collect())
    }
}
