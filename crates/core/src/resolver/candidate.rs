use germ_pms::PackageView;
use log::{debug, info, trace, warn};

use super::outcome::{CandidateRejectionReason, RejectedCandidate, RequirementFailure};
use super::provider::PackageLookup;
use super::requirement::AtomRequirement;
use super::state::PackageId;
use super::{EffectivePackage, Resolver};
use crate::policy::PolicyResult;

impl<U: PackageLookup> Resolver<U> {
    /// Resolves all candidate for the given [`AtomRequirement`] requirement.
    ///
    /// Updates the resolver state to reflect selected and rejected candidates.
    /// It evaluates each candidate and recursively resolves its dependencies and
    /// returns when the first candidate has been fully resolved.
    ///
    /// If no suitable candidate could be resolved, the failure is returned in [`RequirementFailure`].
    pub async fn resolve_candidates(
        &mut self,
        requirement: AtomRequirement<'_>,
    ) -> anyhow::Result<Option<RequirementFailure>> {
        let atom = requirement.atom();
        if self.state.already_satisfied(&requirement)? {
            trace!("already satisfied: {atom}");
            return Ok(None);
        }

        let pkgs = self.provider.repo_match_by_atom(atom).await?;
        if pkgs.is_empty() {
            return Ok(Some(RequirementFailure::NoCandidate(atom.clone())));
        }

        let mut rejected = Vec::new();
        for pkg in pkgs {
            let id = PackageId::new(&pkg);

            if let Some(rejection) = self.state.rejection(&id).copied() {
                trace!("skipping {}: cached rejection {rejection}", pkg.cpv());
                rejected.push(RejectedCandidate::policy(pkg, rejection));
                continue;
            }

            if self.state.is_selected(&id) || self.state.is_visiting(&id) {
                if self.state.satisfies(&id, &requirement)? {
                    trace!("already selected: {pkg}");
                    return Ok(None);
                }
                rejected.push(RejectedCandidate::requirement(pkg, atom.clone()));
                continue;
            }

            match self.policy.eval(&pkg) {
                Ok(PolicyResult::Accepted(effective_use)) => {
                    let candidate = EffectivePackage::new(pkg, effective_use);
                    match self.eval_candidate(&requirement, &candidate).await? {
                        None => return Ok(None),
                        Some(reason) => {
                            rejected.push(RejectedCandidate::new(candidate.pkg, reason));
                        }
                    }
                }
                Ok(PolicyResult::Rejected(rejection)) => {
                    self.state.reject(id, rejection);
                    rejected.push(RejectedCandidate::policy(pkg, rejection));
                }
                Err(error) => {
                    warn!("skipping unavailable pkg {}: {error:#}", pkg.cpv());
                }
            }
        }

        Ok(Some(RequirementFailure::Exhausted(atom.clone(), rejected)))
    }

    /// Evaluates an [`EffectivePackage`] candidate including its dependencies and
    /// takes care of updating the resolver state.
    ///
    /// The caller must ensure the candidate is compliant with the policy
    /// and is not already selected or currently being visited.
    async fn eval_candidate(
        &mut self,
        requirement: &AtomRequirement<'_>,
        candidate: &EffectivePackage,
    ) -> anyhow::Result<Option<CandidateRejectionReason>> {
        let id = PackageId::new(&candidate.pkg);

        if !requirement.satisfied_by(&candidate.pkg, &candidate.effective_use)? {
            return Ok(Some(CandidateRejectionReason::Requirement(
                requirement.atom().clone(),
            )));
        }
        if let Some(conflict) = self.state.slot_conflict(&candidate.pkg) {
            debug!(
                "skipping {}: slot conflict with {}",
                candidate.pkg, conflict.pkg
            );
            return Ok(Some(CandidateRejectionReason::SlotConflict {
                selected: conflict.pkg.clone().into(),
            }));
        }
        if let Some((atom, owner)) = self.state.matching_active_blocker(candidate)? {
            info!("skipping {} due to active blocker", candidate.pkg);
            return Ok(Some(CandidateRejectionReason::ActiveBlocker {
                atom: atom.to_owned(),
                owner: owner.to_owned().into(),
            }));
        }

        debug!("resolving dependencies for {}", candidate.pkg);
        let traversal = self.start_traversal();
        self.state.visit(id.clone(), candidate);

        match self.resolve_dependencies(candidate).await {
            Ok(None) => match self.provider.vdb_match_by_pkg(&candidate.pkg).await {
                Ok(installed) => {
                    if let Some(installed) = installed {
                        self.state.insert_replacement(id.clone(), installed);
                    }
                    self.state.select(id);
                    Ok(None)
                }
                Err(error) => {
                    traversal.rollback(self);
                    Err(error)
                }
            },
            Ok(Some((kind, failure))) => {
                traversal.rollback(self);
                Ok(Some(CandidateRejectionReason::Dependency(
                    kind,
                    failure.into(),
                )))
            }
            Err(error) => {
                traversal.rollback(self);
                Err(error)
            }
        }
    }
}
