use std::future::{Future, ready};

use anyhow::anyhow;

use super::PkgProvider;
use crate::atom::Atom;
use crate::package::{Package, PackageView};
use crate::policy::PolicyResult;

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

pub struct TestPkgProvider {
    pkgs: Vec<TestPkg>,
}

impl TestPkgProvider {
    pub fn new(pkgs: impl IntoIterator<Item = TestPkg>) -> Self {
        Self {
            pkgs: pkgs.into_iter().collect(),
        }
    }
}

impl PkgProvider for TestPkgProvider {
    fn find(
        &self,
        atom: &Atom,
    ) -> impl Future<Output = anyhow::Result<Vec<anyhow::Result<Package>>>> + Send {
        ready(Ok(self
            .pkgs
            .iter()
            .filter(|p| p.pkg.matches_atom(atom))
            .map(|p| Ok(p.pkg.clone()))
            .collect()))
    }

    fn eval(&self, pkg: &Package) -> impl Future<Output = anyhow::Result<PolicyResult>> + Send {
        ready(
            self.pkgs
                .iter()
                .find(|p| p.pkg == *pkg)
                .map(|p| p.result.clone())
                .ok_or_else(|| anyhow!("missing test policy result for {pkg}")),
        )
    }
}
