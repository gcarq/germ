use crate::deps::atom::Atom;
use crate::deps::{PrefixedUseFlag, UseFlagPrefix};
use crate::makenv::{EnvValue, MakeEnv};
use crate::repository::set::RepoSet;
use crate::vdb::Vdb;
use crate::vdb::package::InstalledPackage;
use anyhow::Context;
use std::path::PathBuf;
use std::str::FromStr;

/// Prints system- and package information for all packages matching the given `Atom`.
pub fn info(atom: &Option<Atom>, repo_set: &RepoSet, make_env: &MakeEnv) -> anyhow::Result<()> {
    println!("Repositories:\n\n{repo_set}");
    let mut make_env = make_env.iter().collect::<Vec<(&String, &EnvValue)>>();
    make_env.sort_by_key(|(name, _)| *name);
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
    let mut flags = package
        .metadata
        .iuse
        .iter()
        .map(|iuse| match package.use_flags.contains(iuse.inner()) {
            true => PrefixedUseFlag::from_parts(UseFlagPrefix::None, iuse.inner().clone()),
            false => PrefixedUseFlag::from_parts(UseFlagPrefix::Disable, iuse.inner().clone()),
        })
        .collect::<Vec<_>>();
    flags.sort();

    println!(
        "USE=\"{}\"",
        flags
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ")
    );
}
