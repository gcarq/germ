use anyhow::anyhow;
use germ_core::repository::{PackageResolutionError, RepoSet};
use log::{info, warn};

/// Generates metadata cache for repositories.
pub fn gencache(repo_name: Option<&str>, repo_set: &mut RepoSet) -> anyhow::Result<()> {
    for repo in repo_set.select_mut(repo_name) {
        let name = repo.name.clone();
        info!("Generating metadata cache for {name}...");
        for error in repo.build_cache() {
            match error {
                err @ PackageResolutionError::Metadata { .. } => warn!("{name}: {err}"),
                err @ PackageResolutionError::Internal { .. } => {
                    return Err(anyhow!(err).context(format!("unable to process repo {name}")));
                }
            }
        }
    }
    Ok(())
}
