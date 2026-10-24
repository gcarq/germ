use std::fmt;

use germ_pms::{Atom, BlockerStrength, MetaVar, Package};

use super::ExecutionPlan;
use super::state::PackageId;
use crate::policy::PolicyRejection;
use crate::types::FxIndexMap;

/// The policy-rejected packages and their rejection reasons, keyed by package key.
pub type RejectedPackages = FxIndexMap<PackageId, PolicyRejection>;

/// The outcome of resolving the root [`Atom`]s.
///
/// A root that cannot be resolved is rolled back and recorded in a
/// [`RequirementFailure`], while the plan keeps the operations of every root
/// that did resolve.
#[derive(Debug)]
pub enum ResolutionOutcome {
    /// Every root atom resolved.
    Complete {
        plan: ExecutionPlan,
        rejected: RejectedPackages,
    },
    /// At least one root atom resolved and at least one failed.
    Partial {
        plan: ExecutionPlan,
        failures: Vec<RequirementFailure>,
        rejected: RejectedPackages,
    },
    /// No root atom resolved.
    Failed {
        failures: Vec<RequirementFailure>,
        rejected: RejectedPackages,
    },
}

impl ResolutionOutcome {
    /// Returns the plan for the roots that resolved.
    pub const fn plan(&self) -> Option<&ExecutionPlan> {
        match self {
            Self::Complete { plan, .. } | Self::Partial { plan, .. } => Some(plan),
            Self::Failed { .. } => None,
        }
    }

    /// Returns every root failure.
    pub fn failures(&self) -> &[RequirementFailure] {
        match self {
            Self::Complete { .. } => &[],
            Self::Partial { failures, .. } | Self::Failed { failures, .. } => failures,
        }
    }

    /// Returns the rejected packages and their rejection reasons.
    pub const fn rejected(&self) -> &RejectedPackages {
        match self {
            Self::Complete { rejected, .. }
            | Self::Partial { rejected, .. }
            | Self::Failed { rejected, .. } => rejected,
        }
    }
}

/// Represents a rejected [`Package`] together with its [`CandidateRejectionReason`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RejectedCandidate {
    pub pkg: Package,
    pub reason: CandidateRejectionReason,
}

impl RejectedCandidate {
    /// Creates a rejected candidate for `pkg` with its `reason`.
    pub const fn new(pkg: Package, reason: CandidateRejectionReason) -> Self {
        Self { pkg, reason }
    }

    /// Creates a rejected candidate for `pkg` due to a policy rejection.
    pub const fn policy(pkg: Package, rejection: PolicyRejection) -> Self {
        let reason = CandidateRejectionReason::Policy(rejection);
        Self { pkg, reason }
    }

    /// Creates a rejected candidate for `pkg` due to an unsatisfied requirement.
    pub const fn requirement(pkg: Package, atom: Atom) -> Self {
        let reason = CandidateRejectionReason::Requirement(atom);
        Self { pkg, reason }
    }
}

/// Describes why the resolver rejected a candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateRejectionReason {
    Policy(PolicyRejection),
    Requirement(Atom),
    SlotConflict { selected: Box<Package> },
    ActiveBlocker { atom: Atom, owner: Box<Package> },
    Dependency(MetaVar, Box<RequirementFailure>),
}

impl fmt::Display for CandidateRejectionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(rejection) => write!(f, "{rejection}"),
            Self::Requirement(atom) => write!(f, "does not satisfy requirement {atom}"),
            Self::SlotConflict { selected } => write!(f, "slot conflict with {selected}"),
            Self::ActiveBlocker { atom, owner } => write!(f, "blocked by {atom} from {owner}"),
            Self::Dependency(field, failure) => write!(f, "unsatisfied {field}: {failure}"),
        }
    }
}

/// A structured reason why an atom requirement or dependency expression
/// could not be satisfied.
///
/// A root failure is always [`Self::NoCandidate`] or [`Self::Exhausted`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequirementFailure {
    NoCandidate(Atom),
    Exhausted(Atom, Vec<RejectedCandidate>),
    Blocker {
        strength: BlockerStrength,
        atom: Atom,
        conflict: Box<Package>,
    },
    AnyOf(Vec<RequirementFailure>),
}

impl fmt::Display for RequirementFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCandidate(atom) => write!(f, "no candidate for {atom}"),
            Self::Exhausted(atom, candidates) => {
                write!(f, "no viable candidate for {atom}")?;
                for candidate in candidates {
                    write!(f, "\n\t{}: {}", candidate.pkg, candidate.reason)?;
                }
                Ok(())
            }
            Self::Blocker {
                strength,
                atom,
                conflict,
            } => match strength {
                BlockerStrength::Weak => write!(f, "!{atom} conflicts with {conflict}"),
                BlockerStrength::Strong => write!(f, "!!{atom} conflicts with {conflict}"),
            },
            Self::AnyOf(failures) => {
                f.write_str("no alternative is satisfiable")?;
                for failure in failures {
                    write!(f, "\n\t{failure}")?;
                }
                Ok(())
            }
        }
    }
}
