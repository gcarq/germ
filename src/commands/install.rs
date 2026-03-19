use crate::deps::atom::Atom;
use crate::repository::set::RepoSet;
use anyhow::{Result, anyhow};

/// Installs the best matching package for the given `atom`.
/// TODO: this is just a placeholder for now.
pub fn install(atom: &Atom, repo_set: &mut RepoSet) -> Result<()> {
    let packages = repo_set.find_packages(atom)?;
    let package = packages
        .first()
        .ok_or_else(|| anyhow!("no matching package found for atom '{atom}'"))?;

    println!("{}", package.metadata);

    Ok(())
}
