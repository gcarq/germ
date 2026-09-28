use log::warn;

use crate::package::{Package, PackageView};
use crate::policy::{PackagePolicy, PolicyResult};
use crate::resolver::{Candidate, CandidateRejection};

pub fn eval_policy(
    pkgs: impl IntoIterator<Item = Package>,
    policy: &PackagePolicy,
) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    for pkg in pkgs {
        let candidate = match policy.eval(&pkg) {
            Ok(PolicyResult::Accepted(effective_use)) => Candidate::Accepted(pkg, effective_use),
            Ok(PolicyResult::Masked) => Candidate::Rejected(pkg, CandidateRejection::Masked),
            Ok(PolicyResult::MissingKeyword) => {
                Candidate::Rejected(pkg, CandidateRejection::MissingKeyword)
            }
            Ok(PolicyResult::RequiredUseUnsatisfied) => {
                Candidate::Rejected(pkg, CandidateRejection::RequiredUseUnsatisfied)
            }
            Err(error) => {
                warn!("skipping unavailable pkg {}: {error:#}", pkg.cpv());
                continue;
            }
        };
        candidates.push(candidate);
    }
    candidates
}
