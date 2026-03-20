use crate::deps::atom::Atom;
use crate::makenv::MakeEnv;
use crate::repository::set::RepoSet;
use crate::vdb::Vdb;
use crate::vdb::package::InstalledPackage;
use anyhow::Context;
use std::path::PathBuf;
use std::str::FromStr;

/// Prints system- and package information for all packages matching the given `Atom`.
pub fn info(atom: &Option<Atom>, repo_set: &RepoSet, make_env: &MakeEnv) -> anyhow::Result<()> {
    println!("Repositories:");
    for repo in repo_set.values() {
        println!(" * {repo} -> {}", repo.location.display());
    }
    println!();

    let mut make_env = make_env
        .iter()
        .map(|(key, value)| (key.clone(), value.to_string()))
        .collect::<Vec<(String, String)>>();
    make_env.sort_by(|a, b| a.0.cmp(&b.0));
    for (key, value) in make_env {
        println!("{key}=\"{value}\"");
    }

    let Some(atom) = atom else { return Ok(()) };
    let vdb =
        Vdb::from_path(PathBuf::from_str("/var/db/pkg")?).with_context(|| "unable to build VDB")?;
    let packages = vdb.find_by_atom(atom);
    println!("\nInstalled packages matching {atom}\n");
    for pkg in packages {
        println!("{pkg}");
        print_use_flags(pkg);
        println!();
    }

    Ok(())
}

/// Prints USE flag usage for the given `package`.
fn print_use_flags(package: &InstalledPackage) {
    let mut enabled = Vec::new();
    let mut disabled = Vec::new();

    for flag in &package.metadata.iuse {
        if package.use_flags.contains(flag.inner()) {
            enabled.push(flag.inner().to_string());
        } else {
            disabled.push(format!("-{}", flag.inner()));
        }
    }
    enabled.sort();
    disabled.sort();

    println!("USE=\"{} {}\"", enabled.join(" "), disabled.join(" "));
}
