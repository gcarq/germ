use anyhow::Context;
use colored::Colorize;
use germ_core::SysConf;
use germ_core::conf::portage::PortageConf;
use germ_core::deps::atom::Atom;
use germ_core::package::PackageView;
use germ_core::repository::RepoSet;
use germ_core::vdb::{Vdb, package::InstalledPackage};
use std::sync::Arc;

/// Prints system- and package information for all packages matching the given `Atom`.
pub fn info(atom: Option<&Atom>, sysconf: &Arc<SysConf>) -> anyhow::Result<()> {
    let reposet = RepoSet::new(sysconf.clone()).context("unable to build repo set")?;
    let conf = PortageConf::new(&reposet, sysconf)?;

    println!("Repositories:");
    for repo in reposet.iter() {
        println!(" * {repo} -> {}", repo.location().display());
    }
    println!();

    let mut env = conf
        .makenv()
        .vars()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect::<Vec<_>>();
    env.sort();
    for (key, value) in env {
        println!("{key}=\"{value}\"");
    }

    let Some(atom) = atom else { return Ok(()) };

    let mut vdb = Vdb::from_path(sysconf.vdb_path()).context("unable to read VDB")?;
    let packages = vdb
        .find_by_atom(atom)
        .with_context(|| format!("unable to find installed packages matching {atom}"))?;
    println!(
        "\nInstalled packages matching {}:\n",
        atom.to_string().bold()
    );
    for pkg in packages {
        println!("{}", pkg.to_string().green().bold());
        print_useflags(pkg);
        println!();
    }

    Ok(())
}

/// Prints USE flag usage for the given `package`.
fn print_useflags(package: &InstalledPackage) {
    let mut enabled = Vec::new();
    let mut disabled = Vec::new();

    for entry in package.metadata().iuse() {
        let flag = entry.flag();
        if package.enabled_useflags().contains(flag) {
            enabled.push(flag);
        } else {
            disabled.push(flag);
        }
    }
    enabled.sort();
    disabled.sort();

    let enabled = enabled
        .iter()
        .map(|flag| format!("{}", flag.to_string().red().bold()));

    let disabled = disabled
        .iter()
        .map(|flag| format!("{}", format!("-{flag}").blue().bold()));

    println!(
        "USE=\"{}\"",
        enabled.chain(disabled).collect::<Vec<_>>().join(" ")
    );
}
