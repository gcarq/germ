use futures_util::future::LocalBoxFuture;
use germ_pms::deps::{DepAtom, Expr, ExprNodes};
use germ_pms::{Atom, BlockerStrength, MetaVar, PackageView};

use super::outcome::RequirementFailure;
use super::provider::PackageLookup;
use super::requirement::AtomRequirement;
use super::{EffectivePackage, Resolver};

impl<U: PackageLookup> Resolver<U> {
    /// Resolves every dependency field for the given `owner` [`EffectivePackage`].
    pub(super) async fn resolve_dependencies(
        &mut self,
        owner: &EffectivePackage,
    ) -> anyhow::Result<Option<(MetaVar, RequirementFailure)>> {
        let metadata = owner.pkg.metadata();
        for (field, tree) in metadata.dependencies() {
            if let Some(failure) = self.resolve_group_all(owner, tree.roots()).await? {
                return Ok(Some((field, failure)));
            }
        }
        Ok(None)
    }

    /// Resolves a single dependency expression.
    fn resolve_expr<'a>(
        &'a mut self,
        owner: &'a EffectivePackage,
        expr: Expr<'a, DepAtom>,
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
        nodes: ExprNodes<'_, DepAtom>,
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
    /// Already satisfied atoms either by a local package, or the resolver state
    /// are tried first.
    async fn resolve_group_any(
        &mut self,
        owner: &EffectivePackage,
        nodes: ExprNodes<'_, DepAtom>,
    ) -> anyhow::Result<Option<RequirementFailure>> {
        let mut preferred = Vec::new();
        let mut rest = Vec::new();
        for expr in nodes {
            if let Expr::Item(atom) = &expr
                && atom.blocker().is_none()
            {
                let requirement = AtomRequirement::dependency(atom.inner(), &owner.effective_use);
                if self.state.already_satisfied(&requirement)?
                    || self.installed_satisfies(&requirement).await?
                {
                    preferred.push(expr);
                    continue;
                }
            }
            rest.push(expr);
        }

        let mut failures = Vec::new();
        for expr in preferred.into_iter().chain(rest) {
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

    /// Returns whether an installed package satisfies `requirement`.
    async fn installed_satisfies(
        &mut self,
        requirement: &AtomRequirement<'_>,
    ) -> anyhow::Result<bool> {
        for pkg in self.provider.vdb_match_by_atom(requirement.atom()).await? {
            if requirement.satisfied_by(&pkg, pkg.effective_use())? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Resolves a dependency item and handles any blockers.
    async fn resolve_atom(
        &mut self,
        owner: &EffectivePackage,
        atom: &DepAtom,
    ) -> anyhow::Result<Option<RequirementFailure>> {
        if let Some(strength) = atom.blocker() {
            self.handle_blocker(owner, atom.inner(), strength).await
        } else {
            let requirement = AtomRequirement::dependency(atom.inner(), &owner.effective_use);
            self.resolve_candidates(requirement).await
        }
    }

    /// Handles the given `atom` with its `strength` and updates the state.
    async fn handle_blocker(
        &mut self,
        owner: &EffectivePackage,
        atom: &Atom,
        strength: BlockerStrength,
    ) -> anyhow::Result<Option<RequirementFailure>> {
        let installed = self.provider.vdb_match_by_atom(atom).await?;
        match self
            .state
            .register_blocker(owner, atom, strength, installed)?
        {
            Some(conflict) => Ok(Some(RequirementFailure::Blocker {
                strength,
                atom: atom.clone(),
                conflict: conflict.into(),
            })),
            None => Ok(None),
        }
    }
}
