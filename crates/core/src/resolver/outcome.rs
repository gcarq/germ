use std::fmt;

use super::{DependencyKind, ExecutionPlan, state::PackageKey};
use crate::package::{AtomRequirement, Package};
use crate::types::FxIndexMap;
use crate::useflag::EffectiveUse;

#[derive(Debug)]
pub struct ResolutionOutcome {
    is_resolved: bool,
    plan: ExecutionPlan,
    rejected: FxIndexMap<PackageKey, CandidateRejection>,
}

impl ResolutionOutcome {
    pub(super) const fn new(
        is_resolved: bool,
        plan: ExecutionPlan,
        rejected: FxIndexMap<PackageKey, CandidateRejection>,
    ) -> Self {
        Self {
            is_resolved,
            plan,
            rejected,
        }
    }

    /// Returns whether the resolution was successful.
    pub const fn is_resolved(&self) -> bool {
        self.is_resolved
    }

    /// Returns the [`ExecutionPlan`].
    pub const fn plan(&self) -> &ExecutionPlan {
        &self.plan
    }

    /// Returns the map of rejected candidates with their reasons.
    pub const fn rejected(&self) -> &FxIndexMap<PackageKey, CandidateRejection> {
        &self.rejected
    }
}

/// A package selected by the resolver together with its effective USE state.
#[derive(Debug, Eq, PartialEq)]
pub struct SelectedPackage {
    pub pkg: Package,
    pub effective_use: EffectiveUse,
}

impl SelectedPackage {
    pub const fn new(pkg: Package, effective_use: EffectiveUse) -> Self {
        Self { pkg, effective_use }
    }

    pub fn matches(&self, request: &AtomRequirement<'_>) -> anyhow::Result<bool> {
        request.satisfied_by(&self.pkg, &self.effective_use)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CandidateRejection {
    MissingKeyword,
    Masked,
    RequiredUseUnsatisfied,
    DependencyUnsatisfied(DependencyKind),
}

impl fmt::Display for CandidateRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingKeyword => f.write_str("missing keyword"),
            Self::Masked => f.write_str("masked"),
            Self::RequiredUseUnsatisfied => f.write_str("required USE unsatisfied"),
            Self::DependencyUnsatisfied(kind) => write!(f, "unsatisfied {kind}"),
        }
    }
}
