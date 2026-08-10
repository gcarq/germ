use anyhow::{Result, anyhow};
use germ_core::deps::atom::Atom;
use germ_core::repository::RepoSet;

/// Installs the best matching package for the given `atom`.
/// TODO: this is just a placeholder for now.
pub async fn install(atom: &Atom, repo_set: &mut RepoSet) -> Result<()> {
    let package = repo_set
        .find_packages(atom)
        .await?
        .into_iter()
        .find_map(Result::ok)
        .ok_or_else(|| anyhow!("no matching package found for atom '{atom}'"))?;

    println!("{}", package.metadata);

    Ok(())
}
