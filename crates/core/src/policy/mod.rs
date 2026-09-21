pub mod keyword;
pub mod pkgmask;
pub mod useflag;

use anyhow::Context;

use self::keyword::EffectiveKeywords;
use self::pkgmask::PackageMasks;
use self::useflag::UsePolicy;
use crate::package::PackageView;
use crate::useflag::EffectiveUse;

/// Defines the outcome of the package evaluation against the policies.
#[derive(Debug, Clone)]
pub enum PolicyResult {
    Accepted(EffectiveUse),
    Masked,
    MissingKeyword,
    RequiredUseUnsatisfied,
}

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

    /// Evaluates a package and returns the result as [`PolicyResult`].
    pub fn evaluate<P>(&self, pkg: &P) -> anyhow::Result<PolicyResult>
    where
        P: PackageView,
    {
        let keyword = self.keywords.evaluate(pkg);
        let (effective_use, use_satisfied) = self
            .usepolicy
            .evaluate(pkg, keyword.stable_in_use)
            .context("failed to evaluate required USE flags")?;

        let result = if !keyword.accepted {
            PolicyResult::MissingKeyword
        } else if !use_satisfied {
            PolicyResult::RequiredUseUnsatisfied
        } else if self.pkgmasks.is_masked(pkg) {
            PolicyResult::Masked
        } else {
            PolicyResult::Accepted(effective_use)
        };
        Ok(result)
    }
}
