mod index;
pub mod keyword;
pub mod pkgmask;
#[cfg(test)]
pub(crate) mod test_support;
pub mod useflag;

use std::fmt;

use anyhow::Context;
use germ_pms::PackageView;

use self::keyword::EffectiveKeywords;
use self::pkgmask::PackageMasks;
use self::useflag::UsePolicy;
use crate::files::entry::LineOrigin;
use crate::policy::keyword::KeywordEvalResult;
use crate::policy::useflag::UseFlagEvalResult;
use crate::useflag::EffectiveUse;

/// Defines the outcome of the package evaluation against the policies.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum PolicyResult {
    Accepted(EffectiveUse),
    Rejected(PolicyRejection),
}

impl From<UseFlagEvalResult> for PolicyResult {
    fn from(result: UseFlagEvalResult) -> Self {
        match result {
            UseFlagEvalResult::Satisfied(effective_use) => Self::Accepted(effective_use),
            UseFlagEvalResult::Unsatisfied => {
                Self::Rejected(PolicyRejection::RequiredUseUnsatisfied)
            }
        }
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum PolicyRejection {
    Masked(LineOrigin),
    MissingKeyword,
    RequiredUseUnsatisfied,
}

impl fmt::Display for PolicyRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingKeyword => f.write_str("missing keyword"),
            Self::Masked(origin) => write!(f, "masked (by {origin})"),
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
        let stable_in_use = match self.keywords.eval(pkg) {
            KeywordEvalResult::Accepted { stable_in_use } => stable_in_use,
            KeywordEvalResult::Rejected => {
                return Ok(PolicyResult::Rejected(PolicyRejection::MissingKeyword));
            }
        };

        if let Some(masked_by) = self.pkgmasks.masking(pkg) {
            return Ok(PolicyResult::Rejected(PolicyRejection::Masked(masked_by)));
        }

        let result = self
            .usepolicy
            .eval(pkg, stable_in_use)
            .context("failed to evaluate required USE flags")?;
        Ok(PolicyResult::from(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::test_support::origin;

    #[test]
    fn test_masked_display() {
        assert_eq!(
            PolicyRejection::Masked(origin(3)).to_string(),
            "masked (by /dev/null:3)"
        );
    }
}
