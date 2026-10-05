pub mod keyword;
pub mod pkgmask;
#[cfg(test)]
pub(crate) mod test_support;
pub mod useflag;

use std::fmt;

use anyhow::Context;

use self::keyword::EffectiveKeywords;
use self::pkgmask::PackageMasks;
use self::useflag::UsePolicy;
use crate::package::PackageView;
use crate::useflag::EffectiveUse;

/// Defines the outcome of the package evaluation against the policies.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum PolicyResult {
    Accepted(EffectiveUse),
    Rejected(PolicyRejection),
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum PolicyRejection {
    Masked,
    MissingKeyword,
    RequiredUseUnsatisfied,
}

impl fmt::Display for PolicyRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingKeyword => f.write_str("missing keyword"),
            Self::Masked => f.write_str("masked"),
            Self::RequiredUseUnsatisfied => f.write_str("required USE unsatisfied"),
        }
    }
}

/// Represents the effective package policy, which is a combination of keywords,
/// USE flags and package masks.
///
/// TODO: Add license policies.
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
    pub fn eval<P>(&self, pkg: &P) -> anyhow::Result<PolicyResult>
    where
        P: PackageView,
    {
        let keyword = self.keywords.eval(pkg);
        if !keyword.accepted {
            return Ok(PolicyResult::Rejected(PolicyRejection::MissingKeyword));
        }

        if self.pkgmasks.is_masked(pkg) {
            return Ok(PolicyResult::Rejected(PolicyRejection::Masked));
        }

        let (effective_use, use_satisfied) = self
            .usepolicy
            .eval(pkg, keyword.stable_in_use)
            .context("failed to evaluate required USE flags")?;

        let result = if use_satisfied {
            PolicyResult::Accepted(effective_use)
        } else {
            PolicyResult::Rejected(PolicyRejection::RequiredUseUnsatisfied)
        };
        Ok(result)
    }
}
