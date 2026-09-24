use std::future::{Future, ready};

use anyhow::anyhow;

use super::installed::VdbPackages;
use super::provider::{Candidate, PkgProvider};
use crate::atom::Atom;
use crate::package::{Package, PackageView};
use crate::policy::PolicyResult;
use crate::vdb::package::InstalledPackage;

/// Holds a [`Package`] with a predefined [`PolicyResult`] for testing.
pub struct TestPkg {
    pkg: Package,
    result: PolicyResult,
}

impl TestPkg {
    pub const fn new(pkg: Package, result: PolicyResult) -> Self {
        Self { pkg, result }
    }
}

/// Holds an unresolved candidate that cannot be resolved.
struct Unavailable {
    cpv: String,
    // `anyhow::Error` doesn't implement `Clone`.
    reason: String,
}

pub struct TestPkgProvider {
    pkgs: Vec<TestPkg>,
    unavailable: Vec<Unavailable>,
}

impl TestPkgProvider {
    pub fn new(pkgs: impl IntoIterator<Item = TestPkg>) -> Self {
        Self {
            pkgs: pkgs.into_iter().collect(),
            unavailable: Vec::default(),
        }
    }

    /// Adds a candidate that couldn't be resolved.
    pub fn with_failure(mut self, cpv: impl Into<String>, reason: impl Into<String>) -> Self {
        self.unavailable.push(Unavailable {
            cpv: cpv.into(),
            reason: reason.into(),
        });
        self
    }
}

impl PkgProvider for TestPkgProvider {
    fn candidates(
        &self,
        atom: &Atom,
    ) -> impl Future<Output = anyhow::Result<Vec<Candidate>>> + Send {
        let unresolved = self.unavailable.iter().map(|entry| Candidate::Unavailable {
            cpv: entry.cpv.clone(),
            error: anyhow!("{}", entry.reason),
        });
        let evaluated = self
            .pkgs
            .iter()
            .filter(|pkg| pkg.pkg.matches_atom(atom))
            .map(|pkg| Candidate::Evaluated {
                pkg: pkg.pkg.clone(),
                policy: pkg.result.clone(),
            });

        ready(Ok(unresolved.chain(evaluated).collect()))
    }
}

/// Mocked VDB interface for testing.
#[derive(Default)]
pub struct TestVdbPackages {
    pkgs: Vec<InstalledPackage>,
}

impl TestVdbPackages {
    pub fn new(pkgs: impl IntoIterator<Item = InstalledPackage>) -> Self {
        Self {
            pkgs: pkgs.into_iter().collect(),
        }
    }
}

impl VdbPackages for TestVdbPackages {
    async fn find_by_atom(&mut self, atom: &Atom) -> anyhow::Result<Vec<InstalledPackage>> {
        Ok(self
            .pkgs
            .iter()
            .filter(|pkg| pkg.matches_atom(atom))
            .cloned()
            .collect())
    }
}
