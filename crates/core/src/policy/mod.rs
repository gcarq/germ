pub mod keyword;
pub mod pkgmask;
pub mod useflag;

use anyhow::Context;

use self::{keyword::EffectiveKeywords, pkgmask::PackageMasks, useflag::UsePolicy};
use crate::package::PackageView;

/// Represents the effective package policy, which is a combination of keywords,
/// USE flags and package masks.
pub struct PackagePolicy {
    keywords: EffectiveKeywords,
    usepolicy: UsePolicy,
    pkgmasks: PackageMasks,
}

impl PackagePolicy {
    pub const fn new(
        keywords: EffectiveKeywords,
        usepolicy: UsePolicy,
        pkgmasks: PackageMasks,
    ) -> Self {
        Self {
            keywords,
            usepolicy,
            pkgmasks,
        }
    }

    /// Evaluates whether the given package is a valid candidate.
    pub fn evaluate<P: PackageView>(&self, pkg: &P) -> anyhow::Result<bool> {
        let keyword = self.keywords.evaluate(pkg);
        let use_satisfied = self
            .usepolicy
            .required_use_satisfied(pkg, keyword.stable_in_use)
            .context("failed to evaluate required USE flags")?;
        Ok(keyword.accepted && !self.pkgmasks.is_masked(pkg) && use_satisfied)
    }
}
