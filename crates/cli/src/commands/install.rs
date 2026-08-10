use std::sync::Arc;

use anyhow::{Context, anyhow};
use germ_core::SysConf;
use germ_core::deps::atom::Atom;
use germ_core::repository::RepoSet;

/// Installs the best matching package for the given `atom`.
/// TODO: this is just a placeholder for now.
pub async fn install(atom: &Atom, sysconf: Arc<SysConf>) -> anyhow::Result<()> {
    let repo_set = RepoSet::new(sysconf).with_context(|| "unable to build repo set")?;
    let package = repo_set
        .find_packages(atom)
        .await?
        .into_iter()
        .find_map(Result::ok)
        .ok_or_else(|| anyhow!("no matching package found for atom '{atom}'"))?;

    println!("{}", package.metadata);

    Ok(())
}
