use crate::conf::PortageConf;
use crate::consts::DEFAULT_USE_PORTAGE_CONF_PATH;
use crate::deps::Atom;
use crate::package::ebuild::process::{EbuildPhase, EbuildProcess};
use crate::vdb::Vdb;
use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use makenv::EnvValue;
use std::path::{Path, PathBuf};
use std::str::FromStr;

mod conf;
mod consts;
pub mod deps;
mod eapi;
mod linefile;
pub mod makenv;
pub mod package;
pub mod process;
mod profile;
mod regex;
mod repository;
mod utils;
mod vdb;

/// Package management tool for Gentoo-like systems.
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Increases verbosity
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Provides information about the system, useful for troubleshooting
    Info {
        /// Package atom e.g. dev-lang/rust
        #[arg(value_name = "atom")]
        atom: Option<Atom>,
    },
    /// Install a package
    Install {
        /// Package atom to install, e.g. dev-lang/rust
        #[arg(value_name = "atom")]
        atom: Atom,
    },
}

fn main() -> Result<()> {
    let conf = PortageConf::new(Path::new(DEFAULT_USE_PORTAGE_CONF_PATH))?;

    let args = Args::parse();
    match args.command {
        Some(Command::Info { atom }) => info(&conf, atom)?,
        Some(Command::Install { atom }) => install(&conf, atom)?,
        None => {}
    }

    Ok(())
}

/// Prints information about the current portage `conf`.
fn info(conf: &PortageConf, atom: Option<Atom>) -> Result<()> {
    println!("Repositories:\n\n{}", conf.repos_conf);
    let mut make_env = conf.make_env.iter().collect::<Vec<(&String, &EnvValue)>>();
    make_env.sort_by_key(|(name, _)| *name);
    for (key, value) in make_env {
        println!("{key}=\"{value}\"");
    }

    if let Some(atom) = atom {
        let vdb = Vdb::from_path(PathBuf::from_str("/var/db/pkg")?)
            .with_context(|| "Unable to build VDB")?;

        println!("\nInstalled packages matching '{atom}':\n");
        for pkg in vdb.packages.iter().filter(|pkg| atom.matches(pkg)) {
            println!("{pkg}");
        }
    }

    Ok(())
}

/// Installs the best matching package for the given `atom`.
/// TODO: this is just a placeholder for now.
fn install(conf: &PortageConf, atom: Atom) -> Result<()> {
    let pkg = conf
        .repos_conf
        .repositories()
        .filter_map(|repo| repo.find_packages(&atom).first().map(|p| (*p).clone()))
        .next();

    let pkg = match pkg {
        Some(pkg) => pkg,
        None => return Err(anyhow!("No matching package found for atom '{atom}'")),
    };

    let mut proc = EbuildProcess::new(&pkg, EbuildPhase::Metadata)
        .with_context(|| format!("Unable to install package '{pkg}'"))?;
    proc.wait()?;

    Ok(())
}
