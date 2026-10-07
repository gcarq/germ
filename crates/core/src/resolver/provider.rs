use germ_pms::{Atom, Package};
use log::warn;

use crate::repository::RepoSet;
use crate::vdb::Vdb;
use crate::vdb::package::InstalledPackage;

/// Provides an interface to fetch packages needed for the resolver.
pub trait PackageLookup {
    /// Returns ordered [`Package`]s that match `atom`.
    fn repo_match_by_atom(
        &self,
        atom: &Atom,
    ) -> impl Future<Output = anyhow::Result<Vec<Package>>> + Send;

    /// Returns ordered [`InstalledPackage`]s that match `atom`.
    fn vdb_match_by_atom(
        &mut self,
        atom: &Atom,
    ) -> impl Future<Output = anyhow::Result<Vec<InstalledPackage>>> + Send;

    /// Returns an [`InstalledPackage`] that matches `pkg`.
    fn vdb_match_by_pkg(
        &mut self,
        pkg: &Package,
    ) -> impl Future<Output = anyhow::Result<Option<InstalledPackage>>> + Send;
}

/// A package provider backed by a repository set.
pub struct PackageProvider<'a> {
    reposet: &'a RepoSet,
    vdb: Vdb,
}

impl<'a> PackageProvider<'a> {
    pub const fn new(reposet: &'a RepoSet, vdb: Vdb) -> Self {
        Self { reposet, vdb }
    }
}

impl PackageLookup for PackageProvider<'_> {
    async fn repo_match_by_atom(&self, atom: &Atom) -> anyhow::Result<Vec<Package>> {
        let pkgs = self
            .reposet
            .find_packages(atom)
            .await?
            .into_iter()
            .filter_map(|result| match result {
                Ok(pkg) => Some(pkg),
                Err(err) => {
                    warn!("skipping erroneous package: {err:#}");
                    None
                }
            })
            .collect();
        Ok(pkgs)
    }

    async fn vdb_match_by_atom(&mut self, atom: &Atom) -> anyhow::Result<Vec<InstalledPackage>> {
        Ok(self.vdb.find_by_atom(atom)?.cloned().collect())
    }

    async fn vdb_match_by_pkg(
        &mut self,
        pkg: &Package,
    ) -> anyhow::Result<Option<InstalledPackage>> {
        Ok(self.vdb.find_by_pkg(pkg)?.cloned())
    }
}
