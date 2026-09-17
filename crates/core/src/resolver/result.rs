use std::fmt;

use crate::package::Package;
use crate::resolver::DependencyKind;

pub struct ResolutionOutcome {
    pub candidates: Vec<Candidate>,
}

pub struct Candidate {
    pub pkg: Package,
    pub result: CandidateResult,
}

pub enum CandidateResult {
    Selected,
    Rejected(CandidateRejection),
    Err(anyhow::Error),
}

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
