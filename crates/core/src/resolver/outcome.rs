use std::fmt;

use super::state::PackageKey;
use super::{DependencyField, ExecutionPlan};
use crate::atom::Atom;
use crate::package::Package;
use crate::policy::PolicyRejection;
use crate::types::FxIndexMap;

/// The outcome of resolving an [`Atom`] with a resolver.
#[derive(Debug)]
pub struct ResolutionOutcome {
    failure: Option<RequirementFailure>,
    plan: ExecutionPlan,
    rejected: FxIndexMap<PackageKey, PolicyRejection>,
}

impl ResolutionOutcome {
    pub const fn new(
        failure: Option<RequirementFailure>,
        plan: ExecutionPlan,
        rejected: FxIndexMap<PackageKey, PolicyRejection>,
    ) -> Self {
        Self {
            failure,
            plan,
            rejected,
        }
    }
    /// Returns whether the resolution was successful.
    pub const fn is_resolved(&self) -> bool {
        self.failure.is_none()
    }

    /// Returns the root resolution failure, if resolution failed.
    pub const fn failure(&self) -> Option<&RequirementFailure> {
        self.failure.as_ref()
    }

    /// Returns the [`ExecutionPlan`].
    pub const fn plan(&self) -> &ExecutionPlan {
        &self.plan
    }

    /// Returns the map of rejected candidates with their reasons.
    pub const fn rejected(&self) -> &FxIndexMap<PackageKey, PolicyRejection> {
        &self.rejected
    }
}

/// Represents a rejected [`Package`] together with its [`CandidateRejectionReason`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RejectedCandidate {
    pub package: Package,
    pub reason: CandidateRejectionReason,
}

impl RejectedCandidate {
    /// Creates a rejected candidate for `package` with its `reason`.
    pub const fn new(package: Package, reason: CandidateRejectionReason) -> Self {
        Self { package, reason }
    }
}

/// Describes why the resolver rejected a candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateRejectionReason {
    Policy(PolicyRejection),
    Requirement(Atom),
    ActiveBlocker { atom: Atom, owner: Box<Package> },
    Dependency(DependencyField, Box<RequirementFailure>),
}

impl fmt::Display for CandidateRejectionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(rejection) => write!(f, "{rejection}"),
            Self::Requirement(atom) => write!(f, "does not satisfy requirement {atom}"),
            Self::ActiveBlocker { atom, owner } => write!(f, "blocked by {atom} from {owner}"),
            Self::Dependency(kind, failure) => write!(f, "unsatisfied {kind}: {failure}"),
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
    WeakBlocker { atom: Atom, conflict: Box<Package> },
    StrongBlocker(Atom),
    AnyOf(Vec<RequirementFailure>),
}

impl fmt::Display for RequirementFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCandidate(atom) => write!(f, "no candidate for {atom}"),
            Self::Exhausted(atom, candidates) => {
                write!(f, "no viable candidate for {atom}")?;
                for candidate in candidates {
                    write!(f, "\n\t{}: {}", candidate.package, candidate.reason)?;
                }
                Ok(())
            }
            Self::WeakBlocker { atom, conflict } => {
                write!(f, "weak blocker {atom} conflicts with {conflict}")
            }
            Self::StrongBlocker(atom) => write!(f, "unsupported strong blocker !!{atom}"),
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
