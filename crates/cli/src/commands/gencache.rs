use anyhow::Context;
use germ_core::repository::RepoSet;
use log::{info, warn};

/// Generates metadata cache for repositories.
pub async fn gencache(
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
                .with_context(|| format!("unable to recreate cache for {name}"))?;
        }
        info!("Generating metadata cache for {name}...");

        for error in repo
            .build_cache()
            .await
            .with_context(|| format!("unable to process repo {name}"))?
        {
            warn!("{name}: {error}");
        }
    }
    Ok(())
}
