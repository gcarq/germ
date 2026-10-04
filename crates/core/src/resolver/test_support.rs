use super::provider::PackageLookup;
use super::{ResolutionOutcome, Resolver};
use crate::atom::Atom;
use crate::package::{Package, PackageView};
use crate::policy::test_support::PolicyFixture;
use crate::test_support::{cpv, pkg_metadata};
use crate::useflag::test_support::effective;
use crate::vdb::package::InstalledPackage;

/// Returns an installed `app-misc/<name>-<version>` package in `slot` with the given USE state.
pub fn installed(
    name: &str,
    version: &str,
    slot: &str,
    iuse_effective: &[&str],
    enabled: &[&str],
) -> InstalledPackage {
    InstalledPackage::from_parts(
        cpv("app-misc", name, version),
        "gentoo".parse().unwrap(),
        pkg_metadata(&[("SLOT", slot)]),
        effective(iuse_effective, enabled),
    )
}

/// Holds resolver inputs for testing.
pub struct ResolverFixture {
    packages: Vec<Package>,
    installed: Vec<InstalledPackage>,
    policy: PolicyFixture,
}

impl ResolverFixture {
    /// Creates a fixture with `packages` and no installed packages.
    pub fn new(packages: impl IntoIterator<Item = Package>) -> Self {
        Self {
            packages: packages.into_iter().collect(),
            installed: Vec::default(),
            policy: PolicyFixture::default(),
        }
    }

    /// Sets the installed packages available to resolution.
    pub fn with_installed(mut self, installed: impl IntoIterator<Item = InstalledPackage>) -> Self {
        self.installed = installed.into_iter().collect();
        self
    }

    /// Sets the globally enabled USE flags available to resolution.
    pub fn with_use(mut self, flags: &[&str]) -> Self {
        self.policy = self.policy.with_use(flags);
        self
    }

    /// Adds a package mask atom to the policy used for resolution.
    pub fn with_mask(mut self, atom: &str) -> Self {
        self.policy = self.policy.with_mask(atom);
        self
    }

    /// Resolves `atom` and returns the outcome.
    pub async fn resolve(&self, atom: &Atom) -> ResolutionOutcome {
        let provider = PackageLookupFixture::new(&self.packages, &self.installed);
        let resolver = Resolver::new(provider, self.policy.build().unwrap());
        resolver.resolve(atom).await.unwrap()
    }
}

struct PackageLookupFixture<'a> {
    packages: &'a [Package],
    installed: &'a [InstalledPackage],
}

impl<'a> PackageLookupFixture<'a> {
    const fn new(packages: &'a [Package], installed: &'a [InstalledPackage]) -> Self {
        Self {
            packages,
            installed,
        }
    }
}

impl PackageLookup for PackageLookupFixture<'_> {
    async fn repo_match_by_atom(&self, atom: &Atom) -> anyhow::Result<Vec<Package>> {
        let iter = self.packages.iter().filter(|pkg| pkg.matches_atom(atom));
        Ok(iter.cloned().collect())
    }

    async fn vdb_match_by_atom(&mut self, atom: &Atom) -> anyhow::Result<Vec<InstalledPackage>> {
        let iter = self.installed.iter().filter(|pkg| pkg.matches_atom(atom));
        Ok(iter.cloned().collect())
    }

    async fn vdb_match_by_pkg(
        &mut self,
        pkg: &Package,
    ) -> anyhow::Result<Option<InstalledPackage>> {
        Ok(self
            .installed
            .iter()
            .find(|p| p.matches_package_slot(pkg))
            .cloned())
    }
}
