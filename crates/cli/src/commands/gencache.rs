use anyhow::{Result, bail};
use germ_core::repository::{PackageResolutionError, RepoSet};
use log::{info, warn};

/// Generates metadata cache for repositories.
pub fn gencache(repo_name: Option<&str>, repo_set: &mut RepoSet) -> Result<()> {
    for repo in repo_set.select_mut(repo_name) {
        info!("Generating metadata cache for {repo}...");
        for err in repo.build_package_index() {
            match err {
                error @ PackageResolutionError::Metadata { .. } => warn!("{repo}: {error}"),
                error @ PackageResolutionError::Internal { .. } => {
                    bail!("got internal error for {repo}: {error:?}")
                }
            }
        }
    }
    Ok(())
}
