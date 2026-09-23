use std::future::Future;

use crate::atom::Atom;
use crate::package::{Package, PackageView};
use crate::policy::{PackagePolicy, PolicyResult};
use crate::repository::RepoSet;

/// Provides candidate packages for a requirement.
pub trait PkgProvider: Send {
    /// Returns candidate packages for `atom`, in selection order.
    fn candidates(
        &self,
        atom: &Atom,
    ) -> impl Future<Output = anyhow::Result<Vec<Candidate>>> + Send;
}

/// A package offered by a [`PkgProvider`] for an atom.
#[derive(Debug)]
#[expect(clippy::large_enum_variant, reason = "resolved packages dominate")]
pub enum Candidate {
    /// A package that resolved and passed policy evaluation.
    Evaluated { pkg: Package, policy: PolicyResult },
    /// A package that could not be resolved or evaluated.
    Unavailable { cpv: String, error: anyhow::Error },
}

/// A package provider backed by a repository set.
pub struct RepoPkgProvider<'a> {
    reposet: &'a RepoSet,
    policy: &'a PackagePolicy,
}

impl<'a> RepoPkgProvider<'a> {
    pub const fn new(reposet: &'a RepoSet, policy: &'a PackagePolicy) -> Self {
        Self { reposet, policy }
    }
}

impl PkgProvider for RepoPkgProvider<'_> {
    async fn candidates(&self, atom: &Atom) -> anyhow::Result<Vec<Candidate>> {
        let results = self.reposet.find_packages(atom).await?;
        let mut candidates = Vec::with_capacity(results.len());

        for result in results {
            let pkg = match result {
                Ok(pkg) => pkg,
                Err(err) => {
                    candidates.push(Candidate::Unavailable {
                        cpv: err.cpv.clone(),
                        error: err.into(),
                    });
                    continue;
                }
            };

            match self.policy.eval(&pkg).await {
                Ok(policy) => candidates.push(Candidate::Evaluated { pkg, policy }),
                Err(error) => candidates.push(Candidate::Unavailable {
                    cpv: pkg.cpv().fqn().to_owned(),
                    error,
                }),
            }
        }

        Ok(candidates)
    }
}
