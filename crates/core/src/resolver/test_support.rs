use germ_pms::deps::{Depend, ExprKind};
use germ_pms::test_support::{cpv, pkg_metadata};
use germ_pms::{Atom, Package, PackageView};

use super::provider::PackageLookup;
use super::{
    CandidateRejectionReason, EffectivePackage, PackageOperation, RejectedCandidate,
    RequirementFailure, ResolutionOutcome, Resolver,
};
use crate::policy::test_support::PolicyFixture;
use crate::useflag::EffectiveUse;
use crate::useflag::test_support::effective;
use crate::vdb::package::InstalledPackage;

/// Returns an installed package for `fqn` in `slot` with the given USE state.
pub fn installed(
    fqn: &str,
    slot: &str,
    iuse_effective: &[&str],
    enabled: &[&str],
) -> InstalledPackage {
    InstalledPackage::new_unchecked(
        cpv(fqn),
        "gentoo".parse().unwrap(),
        pkg_metadata(&[("SLOT", slot)]),
        effective(iuse_effective, enabled),
    )
}

/// Returns an effective package for `pkg` with no USE flags.
pub fn effective_pkg(pkg: Package) -> EffectivePackage {
    EffectivePackage::new(pkg, EffectiveUse::default())
}

/// Creates a `Merge` operation for `pkg` with no USE flags.
pub fn merge(pkg: Package) -> PackageOperation {
    PackageOperation::Merge(effective_pkg(pkg))
}

/// Creates a `Merge` operation for `pkg` with the given effective USE state.
pub fn merge_with(pkg: Package, effective_use: EffectiveUse) -> PackageOperation {
    PackageOperation::Merge(EffectivePackage::new(pkg, effective_use))
}

/// Creates a `Replace` operation for `pkg` replacing `installed`.
pub fn replace(pkg: Package, installed: InstalledPackage) -> PackageOperation {
    PackageOperation::Replace(effective_pkg(pkg), installed)
}

/// Creates an `Upgrade` operation for `pkg` upgrading `installed`.
pub fn upgrade(pkg: Package, installed: InstalledPackage) -> PackageOperation {
    PackageOperation::Upgrade(effective_pkg(pkg), installed)
}

/// Creates an `Unmerge` operation for `installed`.
pub fn unmerge(installed: InstalledPackage) -> PackageOperation {
    PackageOperation::Unmerge(installed)
}

/// Creates a rejected candidate for `pkg` whose `Depend::VAR` dependency failed.
pub fn dependency_rejection(pkg: Package, failure: RequirementFailure) -> RejectedCandidate {
    RejectedCandidate::new(
        pkg,
        CandidateRejectionReason::Dependency(Depend::VAR, failure.into()),
    )
}

/// Returns the exhausted root failure for `atom` in which `root` was rejected on a dependency.
pub fn exhausted_root(
    atom: Atom,
    root: Package,
    dep_failure: RequirementFailure,
) -> RequirementFailure {
    RequirementFailure::Exhausted(atom, vec![dependency_rejection(root, dep_failure)])
}

/// Asserts that `outcome` is complete and its plan is `operations`.
#[track_caller]
pub fn assert_complete_plan(outcome: &ResolutionOutcome, operations: &[PackageOperation]) {
    assert!(
        matches!(outcome, ResolutionOutcome::Complete { .. }),
        "{outcome:#?}"
    );
    assert_eq!(outcome.plan().unwrap().operations(), operations);
}

/// Asserts that `outcome` is partial and its plan is `operations`.
#[track_caller]
pub fn assert_partial_plan(outcome: &ResolutionOutcome, operations: &[PackageOperation]) {
    assert!(
        matches!(outcome, ResolutionOutcome::Partial { .. }),
        "{outcome:#?}"
    );
    assert_eq!(outcome.plan().unwrap().operations(), operations);
}

/// Holds resolver inputs for testing.
pub struct ResolverFixture {
    packages: Vec<Package>,
    installed: Vec<InstalledPackage>,
    policy: PolicyFixture,
}

impl ResolverFixture {
    /// Creates a fixture from clones of `packages` and no installed packages.
    pub fn new<'a>(packages: impl IntoIterator<Item = &'a Package>) -> Self {
        Self {
            packages: packages.into_iter().cloned().collect(),
            installed: Vec::default(),
            policy: PolicyFixture::default(),
        }
    }

    /// Sets clones of `installed` as the installed packages available to resolution.
    pub fn with_installed<'a>(
        mut self,
        pkgs: impl IntoIterator<Item = &'a InstalledPackage>,
    ) -> Self {
        self.installed = pkgs.into_iter().cloned().collect();
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

    /// Resolves `atoms` and returns the outcome.
    pub async fn resolve<'a>(
        &self,
        atoms: impl IntoIterator<Item = &'a Atom>,
    ) -> ResolutionOutcome {
        let provider = PackageLookupFixture::new(&self.packages, &self.installed);
        let resolver = Resolver::new(provider, self.policy.build().unwrap());
        resolver.resolve(atoms).await.unwrap()
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
