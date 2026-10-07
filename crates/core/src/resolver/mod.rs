mod candidate;
mod deps;
mod outcome;
mod plan;
mod provider;
mod requirement;
mod speculative;
mod state;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

use germ_pms::Atom;
use log::info;

pub use self::outcome::{
    CandidateRejectionReason, RejectedCandidate, RequirementFailure, ResolutionOutcome,
};
pub use self::plan::{ExecutionPlan, PackageOperation};
pub use self::provider::{PackageLookup, PackageProvider};
pub use self::requirement::AtomRequirement;
use self::state::ResolverState;
use crate::package::Package;
use crate::policy::PackagePolicy;
use crate::useflag::EffectiveUse;

/// Orchestrates dependency resolution for [`Package`](crate::package::Package)s.
///
/// Packages are fetched from the given [`PackageLookup`]
/// and evaluated against [`PackagePolicy`].
pub struct Resolver<U> {
    provider: U,
    policy: PackagePolicy,
    state: ResolverState,
}

impl<U: PackageLookup> Resolver<U> {
    /// Creates a new resolver for the given [`PackageLookup`] and [`PackagePolicy`].
    pub fn new(provider: U, policy: PackagePolicy) -> Self {
        Self {
            provider,
            policy,
            state: ResolverState::default(),
        }
    }

    /// Resolves the given root [`Atom`]s and returns a [`ResolutionOutcome`].
    ///
    /// All roots share one [`ResolverState`] and are treated as a single transaction.
    pub async fn resolve<'a>(
        mut self,
        atoms: impl IntoIterator<Item = &'a Atom>,
    ) -> anyhow::Result<ResolutionOutcome> {
        let traversal = self.start_traversal();
        for atom in atoms {
            info!("Resolving candidates for {atom}...");

            if let Some(failure) = self.resolve_candidates(AtomRequirement::root(atom)).await? {
                traversal.rollback(&mut self);
                return Ok(self.state.finalize(Some(failure)));
            }
        }

        Ok(self.state.finalize(None))
    }
}

/// A package together with the effective USE state it was resolved with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectivePackage {
    pub pkg: Package,
    pub effective_use: EffectiveUse,
}

impl EffectivePackage {
    pub const fn new(pkg: Package, effective_use: EffectiveUse) -> Self {
        Self { pkg, effective_use }
    }

    /// Returns true if the package satisfies the given [`AtomRequirement`].
    pub fn matches(&self, requirement: &AtomRequirement<'_>) -> anyhow::Result<bool> {
        requirement.satisfied_by(&self.pkg, &self.effective_use)
    }
}
