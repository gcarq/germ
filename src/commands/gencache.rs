use crate::repository::set::RepoSet;
use anyhow::{Context, Result, anyhow};

/// Generates metadata cache for repositories.
pub fn gencache(repo_name: Option<&String>, repo_set: &mut RepoSet) -> Result<()> {
    if let Some(repo) = repo_name {
        let repo = repo_set
            .get_mut(repo)
            .ok_or_else(|| anyhow!("repository '{repo}' doesn't exist"))?;
        repo.build_package_index()
            .with_context(|| anyhow!("unable to build package index for {repo}"))?;
        return Ok(());
    }

    for repo in repo_set.values_mut() {
        repo.build_package_index()
            .with_context(|| anyhow!("unable to build package index for {repo}"))?;
    }

    Ok(())
}
