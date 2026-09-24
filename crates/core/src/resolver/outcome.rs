use std::fmt;

use super::{DependencyKind, state::PackageKey};
use crate::package::{AtomRequirement, Package};
use crate::types::{FxIndexMap, FxIndexSet};
use crate::useflag::EffectiveUse;
use crate::vdb::package::InstalledPackage;

#[derive(Debug)]
pub struct ResolutionOutcome {
    resolved: bool,
    selected: Vec<SelectedPackage>,
    removals: FxIndexSet<InstalledPackage>,
    rejected: FxIndexMap<PackageKey, CandidateRejection>,
}

impl ResolutionOutcome {
    pub(super) const fn new(
        resolved: bool,
        selected: Vec<SelectedPackage>,
        removals: FxIndexSet<InstalledPackage>,
        rejected: FxIndexMap<PackageKey, CandidateRejection>,
    ) -> Self {
        Self {
            resolved,
            selected,
            removals,
            rejected,
        }
    }

    /// Returns whether the resolution was successful.
    pub const fn is_resolved(&self) -> bool {
        self.resolved
    }

    /// Returns the list of selected packages.
    pub fn selected(&self) -> &[SelectedPackage] {
        &self.selected
    }

    /// Returns the installed packages that are planned for removal.
    pub const fn removals(&self) -> &FxIndexSet<InstalledPackage> {
        &self.removals
    }

    /// Returns the map of rejected candidates with their reasons.
    pub const fn rejected(&self) -> &FxIndexMap<PackageKey, CandidateRejection> {
        &self.rejected
    }
}

/// A package selected by the resolver together with its effective USE state.
#[derive(Debug)]
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

#[derive(Debug)]
pub enum CandidateRejection {
    MissingKeyword,
    Masked,
    RequiredUseUnsatisfied,
    DependencyUnsatisfied(DependencyKind),
    Err(anyhow::Error),
}

impl fmt::Display for CandidateRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingKeyword => f.write_str("missing keyword"),
            Self::Masked => f.write_str("masked"),
            Self::RequiredUseUnsatisfied => f.write_str("required USE unsatisfied"),
            Self::DependencyUnsatisfied(kind) => write!(f, "unsatisfied {kind}"),
            Self::Err(err) => write!(f, "error: {err}"),
        }
    }
}
