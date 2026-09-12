use std::sync::Arc;

use anyhow::Context;
use either::Either;
use germ_core::{SysConf, repository::RepoSet};
use log::{info, warn};

/// Generates metadata cache for repositories.
pub async fn gencache(
    repo_name: Option<&str>,
    force: bool,
    sysconf: Arc<SysConf>,
) -> anyhow::Result<()> {
    if force {
        info!("Forcing cache recreation...");
    }

    let mut reposet = RepoSet::new(sysconf).context("unable to build repo set")?;
    let repos = match repo_name {
        Some(name) => Either::Left(reposet.get_mut(name).into_iter()),
        None => Either::Right(reposet.iter_mut()),
    };

    for repo in repos {
        let name = repo.name().clone();
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
