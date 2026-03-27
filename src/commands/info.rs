use anyhow::{Context, Result};
use colored::Colorize;
use lib::conf::PortageConf;
use lib::deps::atom::Atom;
use lib::repository::set::RepoSet;
use lib::vdb::Vdb;
use lib::vdb::package::InstalledPackage;
use std::path::PathBuf;
use std::str::FromStr;

/// Prints system- and package information for all packages matching the given `Atom`.
pub fn info(atom: Option<&Atom>, repo_set: &RepoSet, conf: &PortageConf) -> Result<()> {
    println!("Repositories:");
    for repo in repo_set.values() {
        println!(" * {repo} -> {}", repo.location.display());
    }
    println!();

    let mut env = conf
        .make_env
        .iter()
        .map(|(key, value)| (key, value.to_string()))
        .collect::<Vec<_>>();
    env.sort_by(|a, b| a.0.cmp(b.0));
    for (key, value) in env {
        println!("{key}=\"{value}\"");
    }

    let Some(atom) = atom else { return Ok(()) };
    let vdb =
        Vdb::from_path(PathBuf::from_str("/var/db/pkg")?).with_context(|| "unable to build VDB")?;
    let packages = vdb.find_by_atom(atom);
    println!(
        "\nInstalled packages matching {}:\n",
        atom.to_string().bold()
    );
    for pkg in packages {
        println!("{}", pkg.to_string().green().bold());
        print_use_flags(pkg, conf);
        println!();
    }

    Ok(())
}

/// Prints USE flag usage for the given `package`.
fn print_use_flags(package: &InstalledPackage, conf: &PortageConf) {
    let mut enabled = Vec::new();
    let mut disabled = Vec::new();

    for flag in &package.metadata.iuse {
        let flag = flag.inner();
        if package.use_flags.contains(flag) {
            enabled.push(flag);
        } else {
            disabled.push(flag);
        }
    }
    enabled.sort();
    disabled.sort();

    let enabled =
        enabled.iter().map(
            |flag| match conf.use_masks.is_forced_for_pkg(package, flag) {
                true => format!("({})", flag.to_string().red().bold()),
                false => format!("{}", flag.to_string().red().bold()),
            },
        );

    let disabled =
        disabled.iter().map(
            |flag| match conf.use_masks.is_masked_for_pkg(package, flag) {
                true => format!("({})", format!("-{flag}").blue().bold()),
                false => format!("{}", format!("-{flag}").blue().bold()),
            },
        );

    println!(
        "USE=\"{}\"",
        enabled.chain(disabled).collect::<Vec<_>>().join(" ")
    );
}
