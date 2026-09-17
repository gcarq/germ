pub mod result;

use std::fmt;

use anyhow::Context;
use futures_util::future::BoxFuture;
use log::{debug, info, warn};

use crate::atom::Atom;
use crate::deps::AtomDep;
use crate::deps::expression::{Expression, ExpressionNodes};
use crate::package::cpv::CPV;
use crate::package::{Package, PackageView};
use crate::policy::{PackagePolicy, PolicyResult, useflag::EffectiveUse};
use crate::repository::{RepoName, RepoSet};
use crate::resolver::result::{CandidateRejection, CandidateResult};
use crate::types::FxHashMap;
use crate::vdb::Vdb;

/// Represents a unique key for a package based on its CPV and repo name.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct PackageKey((CPV, RepoName));

impl PackageKey {
    fn new(pkg: &Package) -> Self {
        Self((pkg.cpv().clone(), pkg.repo().clone()))
    }
}

/// Represents the state of a package during dependency resolution.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
enum PackageState {
    Visiting,
    Satisfied,
    Unsatisfied,
}

/// Identifies the kind of dependency being evaluated for a package.
pub enum DependencyKind {
    Depend,
    BDepend,
    IDepend,
    RDepend,
    PDepend,
}

impl fmt::Display for DependencyKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Depend => f.write_str("DEPEND"),
            Self::BDepend => f.write_str("BDEPEND"),
            Self::IDepend => f.write_str("IDEPEND"),
            Self::RDepend => f.write_str("RDEPEND"),
            Self::PDepend => f.write_str("PDEPEND"),
        }
    }
}

pub struct Resolver<'a> {
    vdb: &'a mut Vdb,
    reposet: &'a RepoSet,
    policy: &'a PackagePolicy,
    pkg_states: FxHashMap<PackageKey, PackageState>,
}

impl<'a> Resolver<'a> {
    pub fn new(vdb: &'a mut Vdb, reposet: &'a RepoSet, policy: &'a PackagePolicy) -> Self {
        Self {
            vdb,
            reposet,
            policy,
            pkg_states: FxHashMap::default(),
        }
    }

    pub async fn resolve(&mut self, atom: &Atom) -> anyhow::Result<bool> {
        for pkg in self.reposet.find_packages(atom).await? {
            let pkg = match pkg {
                Ok(pkg) => pkg,
                Err(err) => {
                    // TODO: format error
                    warn!("{err}");
                    continue;
                }
            };
            if self.eval_candidate(&pkg).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Evaluates a package candidate including its dependencies.
    async fn eval_candidate(&mut self, pkg: &Package) -> anyhow::Result<bool> {
        let key = PackageKey::new(pkg);
        match self.pkg_states.get(&key) {
            Some(PackageState::Visiting | PackageState::Satisfied) => {
                return Ok(true);
            }
            Some(PackageState::Unsatisfied) => {
                return Ok(false);
            }
            None => (),
        }

        match self.policy.evaluate(pkg)? {
            PolicyResult::Accepted(effective_use) => {
                self.pkg_states.insert(key.clone(), PackageState::Visiting);

                info!("evaluating deps for {pkg}");
                match self.eval_dependencies(pkg, &effective_use).await {
                    CandidateResult::Selected => {
                        if let Some(state) = self.pkg_states.get_mut(&key) {
                            *state = PackageState::Satisfied;
                        }
                        return Ok(true);
                    }
                    CandidateResult::Rejected(reason) => {
                        warn!("skipping {pkg} due to {reason}");
                        if let Some(state) = self.pkg_states.get_mut(&key) {
                            *state = PackageState::Unsatisfied;
                        }
                    }
                    CandidateResult::Err(err) => {
                        if let Some(state) = self.pkg_states.get_mut(&key) {
                            *state = PackageState::Unsatisfied;
                        }
                        Err(err).with_context(|| format!("failed to evaluate candidate {pkg}"))?;
                    }
                }
            }
            PolicyResult::Masked => {
                debug!("skipping {pkg} due to package mask");
            }
            PolicyResult::MissingKeyword => {
                debug!("skipping {pkg} due to missing keyword");
            }
            PolicyResult::RequiredUseUnsatisfied => {
                debug!("skipping {pkg} due to unsatisfied required USE flags");
            }
        }
        Ok(false)
    }

    /// Evaluates the all dependencies of a package.
    async fn eval_dependencies<'b>(
        &mut self,
        pkg: &'b Package,
        state: &EffectiveUse<'b>,
    ) -> CandidateResult {
        let dependencies = [
            (DependencyKind::Depend, pkg.metadata().depend().view()),
            (DependencyKind::BDepend, pkg.metadata().bdepend().view()),
            (DependencyKind::IDepend, pkg.metadata().idepend().view()),
            (DependencyKind::RDepend, pkg.metadata().rdepend().view()),
            (DependencyKind::PDepend, pkg.metadata().pdepend().view()),
        ];
        for (kind, exprtree) in dependencies {
            for node in exprtree.roots() {
                match self.eval_expression(node.expression(), state).await {
                    Ok(true) => (),
                    Ok(false) => {
                        return CandidateResult::Rejected(
                            CandidateRejection::DependencyUnsatisfied(kind),
                        );
                    }
                    Err(err) => return CandidateResult::Err(err),
                }
            }
        }
        CandidateResult::Selected
    }

    /// Evaluates `expr` from a package dependency expression against the
    /// package's effective USE state.
    fn eval_expression<'b>(
        &'b mut self,
        expr: Expression<'b, AtomDep>,
        state: &'b EffectiveUse<'b>,
    ) -> BoxFuture<'b, anyhow::Result<bool>> {
        Box::pin(async move {
            match expr {
                Expression::Item(atom) => match atom.blocker() {
                    Some(_) => Ok(!self.vdb.is_installed(atom.inner())?),
                    None => self.resolve(atom.inner()).await,
                },
                Expression::AllOf(nodes) => {
                    for node in nodes {
                        if !self.eval_expression(node.expression(), state).await? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                Expression::AnyOf(nodes) => {
                    for node in nodes {
                        if self.eval_expression(node.expression(), state).await? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                Expression::ExactlyOneOf(nodes) => {
                    Ok(self.eval_up_to_two(nodes, state).await? == 1)
                }
                Expression::AtMostOneOf(nodes) => Ok(self.eval_up_to_two(nodes, state).await? < 2),
                Expression::Use {
                    flag,
                    negated,
                    nodes,
                } => {
                    if state.is_enabled(flag)? == negated {
                        return Ok(true);
                    }
                    debug!("evaluating USE expression for {flag}");
                    for node in nodes {
                        if !self.eval_expression(node.expression(), state).await? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
            }
        })
    }

    /// Evaluates `nodes` and counts how many are satisfied by the given `state`.
    ///
    /// Breaks early if more than two nodes are satisfied.
    async fn eval_up_to_two<'b>(
        &mut self,
        nodes: ExpressionNodes<'b, AtomDep>,
        state: &'b EffectiveUse<'b>,
    ) -> anyhow::Result<u8> {
        let mut count = 0;
        for node in nodes {
            if self.eval_expression(node.expression(), state).await? {
                count += 1;
                if count > 2 {
                    break;
                }
            }
        }
        Ok(count)
    }
}
