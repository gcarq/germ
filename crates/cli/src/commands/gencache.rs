use anyhow::Context;
use germ_core::repository::{PackageResolutionError, RepoSet};
use log::{info, warn};

/// Generates metadata cache for repositories.
pub fn gencache(
    repo_name: Option<&str>,
    force: bool,
    repo_set: &mut RepoSet,
) -> anyhow::Result<()> {
    if force {
        info!("Forcing cache recreation...");
    }
    for repo in repo_set.select_mut(repo_name) {
        let name = repo.name.clone();
        if force {
            repo.recreate_cache()
                .with_context(|| format!("unable recreate cache for {name}"))?;
        }
        info!("Generating metadata cache for {name}...");
        for error in repo.build_cache() {
            match error {
                err @ PackageResolutionError::Metadata { .. } => warn!("{name}: {err}"),
                err @ PackageResolutionError::Internal { .. } => {
                    return Err(err).context(format!("unable to process repo {name}"));
                }
            }
        }
    }
    Ok(())
}
