use futures_util::future::LocalBoxFuture;
use germ_pms::{Atom, AtomBlocker, AtomDep, DependencyField, Expr, ExprNodes};
use log::debug;

use super::outcome::RequirementFailure;
use super::provider::PackageLookup;
use super::{EffectivePackage, Resolver};
use crate::package::{AtomRequirement, PackageView};

impl<U: PackageLookup> Resolver<U> {
    /// Resolves every dependency field for the given `owner` [`EffectivePackage`].
    pub(super) async fn resolve_dependencies(
        &mut self,
        owner: &EffectivePackage,
    ) -> anyhow::Result<Option<(DependencyField, RequirementFailure)>> {
        let metadata = owner.pkg.metadata();
        let dependencies = [
            (DependencyField::Depend, metadata.depend().view()),
            (DependencyField::BDepend, metadata.bdepend().view()),
            (DependencyField::IDepend, metadata.idepend().view()),
            (DependencyField::RDepend, metadata.rdepend().view()),
            (DependencyField::PDepend, metadata.pdepend().view()),
        ];
        for (kind, tree) in dependencies {
            if let Some(failure) = self.resolve_group_all(owner, tree.roots()).await? {
                return Ok(Some((kind, failure)));
            }
        }
        Ok(None)
    }

    /// Resolves a single dependency expression.
    fn resolve_expr<'a>(
        &'a mut self,
        owner: &'a EffectivePackage,
        expr: Expr<'a, AtomDep>,
    ) -> LocalBoxFuture<'a, anyhow::Result<Option<RequirementFailure>>> {
        Box::pin(async move {
            match expr {
                Expr::Item(atom) => self.resolve_atom(owner, atom).await,
                Expr::AllOf(nodes) => self.resolve_group_all(owner, nodes).await,
                Expr::AnyOf(nodes) => self.resolve_group_any(owner, nodes).await,
                Expr::Use {
                    flag,
                    negated,
                    nodes,
                } => match owner.effective_use.is_enabled(flag)? == negated {
                    true => Ok(None),
                    false => self.resolve_group_all(owner, nodes).await,
                },
                Expr::ExactlyOneOf(_) => {
                    unreachable!("BUG: dep expressions cannot contain ^^ groups: {expr}")
                }
                Expr::AtMostOneOf(_) => {
                    unreachable!("BUG: dep expressions cannot contain ?? groups: {expr}")
                }
            }
        })
    }

    /// Resolves every expression in an all-of group.
    async fn resolve_group_all(
        &mut self,
        owner: &EffectivePackage,
        nodes: ExprNodes<'_, AtomDep>,
    ) -> anyhow::Result<Option<RequirementFailure>> {
        for expr in nodes {
            if let Some(failure) = self.resolve_expr(owner, expr).await? {
                return Ok(Some(failure));
            }
        }
        Ok(None)
    }

    /// Resolves alternatives in an any-of group until one succeeds.
    ///
    /// Already satisfied dependencies are preferred.
    async fn resolve_group_any(
        &mut self,
        owner: &EffectivePackage,
        nodes: ExprNodes<'_, AtomDep>,
    ) -> anyhow::Result<Option<RequirementFailure>> {
        for expr in nodes.clone() {
            if let Expr::Item(atom) = expr {
                let requirement = AtomRequirement::dependency(atom.inner(), &owner.effective_use);
                if self.state.already_satisfied(&requirement)? {
                    return Ok(None);
                }
            }
        }

        let mut failures = Vec::new();
        for expr in nodes {
            let traversal = self.start_traversal();
            match self.resolve_expr(owner, expr).await {
                Ok(None) => return Ok(None),
                Ok(Some(failure)) => {
                    traversal.rollback(self);
                    failures.push(failure);
                }
                Err(error) => {
                    traversal.rollback(self);
                    return Err(error);
                }
            }
        }
        Ok(Some(RequirementFailure::AnyOf(failures)))
    }

    /// Resolves a dependency item and handles any blockers.
    async fn resolve_atom(
        &mut self,
        owner: &EffectivePackage,
        atom: &AtomDep,
    ) -> anyhow::Result<Option<RequirementFailure>> {
        if let Some(blocker) = atom.blocker() {
            self.handle_blocker(owner, atom.inner(), blocker).await
        } else {
            let requirement = AtomRequirement::dependency(atom.inner(), &owner.effective_use);
            self.resolve_candidates(requirement).await
        }
    }

    /// Handles the given `atom` with its `blocker` and updates the state.
    async fn handle_blocker(
        &mut self,
        owner: &EffectivePackage,
        atom: &Atom,
        blocker: AtomBlocker,
    ) -> anyhow::Result<Option<RequirementFailure>> {
        match blocker {
            AtomBlocker::Weak => {
                if let Some(conflict) = self.state.weak_blocker_conflict(owner, atom)? {
                    return Ok(Some(RequirementFailure::WeakBlocker {
                        atom: atom.clone(),
                        conflict: conflict.clone().into(),
                    }));
                }

                let installed = self.provider.vdb_match_by_atom(atom).await?;
                self.state.register_weak_blocker(owner, atom, installed)?;
                Ok(None)
            }
            AtomBlocker::Strong => {
                // TODO: implement me
                debug!(
                    "{}: cannot satisfy due to strong blocker {blocker}{atom}",
                    owner.pkg
                );
                Ok(Some(RequirementFailure::StrongBlocker(atom.clone())))
            }
        }
    }
}
