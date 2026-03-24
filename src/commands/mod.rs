//! This module contains all CLI subcommands that are available to the user.
mod gencache;
mod info;
mod install;
mod sync;

use crate::commands::gencache::gencache;
use crate::commands::info::info;
use crate::commands::install::install;
use crate::commands::sync::sync;
use crate::conf::PortageConf;
use crate::deps::atom::Atom;
use crate::repository::set::RepoSet;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum Command {
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

    /// Generate metadata cache for ebuild repositories
    Gencache {
        /// Only generate cache for the given repository (defaults to all)
        #[arg(value_name = "repo")]
        repo: Option<String>,
    },

    /// Sync repositories
    Sync,
}

pub fn execute(command: &Command, repo_set: &mut RepoSet, conf: &PortageConf) -> Result<()> {
    match command {
        Command::Info { atom } => info(atom, repo_set, conf)?,
        Command::Install { atom } => install(atom, repo_set)?,
        Command::Gencache { repo } => gencache(repo, repo_set)?,
        Command::Sync => sync(repo_set),
    }
    Ok(())
}
