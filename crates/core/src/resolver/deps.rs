use std::fmt;

use futures_util::future::LocalBoxFuture;
use log::{debug, info, warn};

use super::Resolver;
use super::provider::PackageLookup;
use crate::atom::{Atom, AtomBlocker};
use crate::deps::AtomDep;
use crate::deps::expr::{Expr, ExprNodes};
use crate::package::{AtomRequirement, Package, PackageView};
use crate::useflag::EffectiveUse;

/// Holds the package and dependency kind while resolving.
struct DependencyContext<'a> {
    owner: &'a Package,
    owner_use: &'a EffectiveUse,
    kind: DependencyKind,
}

impl<'a> DependencyContext<'a> {
    const fn new(owner: &'a Package, owner_use: &'a EffectiveUse, kind: DependencyKind) -> Self {
        Self {
            owner,
            owner_use,
            kind,
        }
    }
}

/// Identifies the dependency kind being evaluated.
#[derive(Copy, Clone, Debug)]
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

impl<U: PackageLookup> Resolver<U> {
    /// Resolves every [`DependencyKind`] for the given `owner`.
    pub(super) async fn resolve_dependencies(
        &mut self,
        owner: &Package,
        owner_use: &EffectiveUse,
    ) -> anyhow::Result<Option<DependencyKind>> {
        let dependencies = [
            (DependencyKind::Depend, owner.metadata().depend().view()),
            (DependencyKind::BDepend, owner.metadata().bdepend().view()),
            (DependencyKind::IDepend, owner.metadata().idepend().view()),
            (DependencyKind::RDepend, owner.metadata().rdepend().view()),
            (DependencyKind::PDepend, owner.metadata().pdepend().view()),
        ];
        for (kind, tree) in dependencies {
            let ctx = DependencyContext::new(owner, owner_use, kind);
            if !self.resolve_all(&ctx, tree.roots()).await? {
                return Ok(Some(kind));
            }
        }
        Ok(None)
    }

    /// Resolves a single dependency expression.
    fn resolve_expr<'a>(
        &'a mut self,
        ctx: &'a DependencyContext<'_>,
        expr: Expr<'a, AtomDep>,
    ) -> LocalBoxFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move {
            match expr {
                Expr::Item(atom) => self.resolve_atom(ctx, atom).await,
                Expr::AllOf(nodes) => self.resolve_all(ctx, nodes).await,
                Expr::AnyOf(nodes) => self.resolve_any(ctx, nodes).await,
                Expr::ExactlyOneOf(_) => {
                    unreachable!("dependency expressions cannot contain exactly-one-of groups")
                }
                Expr::AtMostOneOf(_) => {
                    unreachable!("dependency expressions cannot contain at-most-one-of groups")
                }
                Expr::Use {
                    flag,
                    negated,
                    nodes,
                } => match ctx.owner_use.is_enabled(flag)? == negated {
                    true => Ok(true),
                    false => self.resolve_all(ctx, nodes).await,
                },
            }
        })
    }

    /// Resolves every expression in an all-of group.
    async fn resolve_all(
        &mut self,
        ctx: &DependencyContext<'_>,
        nodes: ExprNodes<'_, AtomDep>,
    ) -> anyhow::Result<bool> {
        for expr in nodes {
            if !self.resolve_expr(ctx, expr).await? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Resolves alternatives in an any-of group until one succeeds.
    async fn resolve_any(
        &mut self,
        ctx: &DependencyContext<'_>,
        nodes: ExprNodes<'_, AtomDep>,
    ) -> anyhow::Result<bool> {
        for expr in nodes {
            if self.resolve_expr(ctx, expr).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Resolves a dependency item and handles any blockers.
    async fn resolve_atom(
        &mut self,
        ctx: &DependencyContext<'_>,
        atom: &AtomDep,
    ) -> anyhow::Result<bool> {
        if let Some(blocker) = atom.blocker() {
            return self.handle_blocker(ctx, atom.inner(), blocker).await;
        }
        let resolved = self
            .resolve_candidates(AtomRequirement::dependency(atom.inner(), ctx.owner_use))
            .await?;
        if !resolved {
            debug!(
                "rejected {}:\n\t -> unsatisfied {}: {atom}",
                ctx.owner, ctx.kind
            );
        }
        Ok(resolved)
    }

    /// Handles a package block for the given `atom`.
    async fn handle_blocker(
        &mut self,
        ctx: &DependencyContext<'_>,
        atom: &Atom,
        blocker: AtomBlocker,
    ) -> anyhow::Result<bool> {
        match blocker {
            AtomBlocker::Weak => {
                let req = AtomRequirement::dependency(atom, ctx.owner_use);
                for (_, sel) in self.state.selected().filter(|sel| &sel.1.pkg != ctx.owner) {
                    if sel.matches(&req)? {
                        warn!(
                            "{}\n\tblocker {atom} in {}\n\tmatched selected {}",
                            ctx.owner, ctx.kind, sel.pkg
                        );
                        return Ok(false);
                    }
                }

                for installed in self.provider.vdb_match_by_atom(atom).await? {
                    // Weak blocks against the expressing package's installed version are ignored.
                    if installed.cpv() == ctx.owner.cpv() {
                        continue;
                    }
                    if !req.satisfied_by(&installed, installed.effective_use())? {
                        continue;
                    }
                    info!(
                        "{}: planning removal of {installed} for weak blocker {atom} in {}",
                        ctx.owner, ctx.kind
                    );
                    self.state.insert_removal(installed);
                }

                self.state
                    .insert_blocker(ctx.owner_use.clone(), atom.clone());
                Ok(true)
            }
            AtomBlocker::Strong => {
                debug!(
                    "{}: cannot satisfy due to strong blocker {blocker}{atom} in {}",
                    ctx.owner, ctx.kind
                );
                Ok(false)
            }
        }
    }
}
