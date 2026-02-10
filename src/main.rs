#![warn(
    clippy::too_many_lines,
    clippy::dbg_macro,
    clippy::doc_link_with_quotes,
    clippy::doc_markdown,
    clippy::empty_structs_with_brackets,
    clippy::missing_const_for_fn,
    clippy::missing_panics_doc,
    clippy::disallowed_script_idents,
    clippy::semicolon_if_nothing_returned,
    clippy::shadow_unrelated,
    clippy::similar_names,
    clippy::unused_self,
    clippy::use_debug,
    clippy::used_underscore_binding,
    clippy::useless_let_if_seq,
    clippy::wildcard_dependencies,
    clippy::wildcard_imports
)]

use crate::conf::PortageConf;
use crate::consts::DEFAULT_USE_PORTAGE_CONF_PATH;
use crate::deps::Atom;
use crate::vdb::Vdb;
use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use ebuild::handler::{EbuildPhase, EbuildPhaseHandler};
use makenv::EnvValue;
use std::path::{Path, PathBuf};
use std::str::FromStr;

mod conf;
mod consts;
pub mod deps;
mod eapi;
pub mod ebuild;
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
    /// Sync the repositories
    Sync,
}

fn main() -> Result<()> {
    let conf = PortageConf::new(Path::new(DEFAULT_USE_PORTAGE_CONF_PATH))?;

    let args = Args::parse();
    match args.command {
        Some(Command::Info { atom }) => info(&conf, atom)?,
        Some(Command::Install { atom }) => install(&conf, atom)?,
        Some(Command::Sync) => sync(&conf)?,
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

    let mut handler = EbuildPhaseHandler::new(&pkg, conf, EbuildPhase::Depend)
        .with_context(|| format!("Unable to install package '{pkg}'"))?;
    handler.execute()?;

    Ok(())
}

/// Syncs all repositories defined in the portage `conf`.
fn sync(conf: &PortageConf) -> Result<()> {
    for repo in conf.repos_conf.repositories() {
        repo.sync()?;
    }
    Ok(())
}
