//! This module contains all CLI subcommands that are available to the user.
mod gencache;
mod info;
mod install;

use crate::commands::gencache::gencache;
use crate::commands::info::info;
use crate::commands::install::install;
use anyhow::Result;
use clap::Subcommand;
use pkgrove_core::deps::atom::Atom;
use pkgrove_core::repository::set::RepoSet;

#[derive(Subcommand)]
pub enum Command {
    /// Provides information about the system, useful for troubleshooting
    Info {
        /// Package atom e.g. dev-lang/rust
        #[arg(value_name = "atom")]
        atom: Option<Atom>,
    },
    /// Install a package (this is just a placeholder!)
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

pub fn execute(command: &Command, repo_set: &mut RepoSet) -> Result<()> {
    match command {
        Command::Info { atom } => info(atom.as_ref(), repo_set)?,
        Command::Install { atom } => install(atom, repo_set)?,
        Command::Gencache { repo } => gencache(repo.as_ref(), repo_set)?,
        Command::Sync => repo_set.maybe_sync()?,
    }
    Ok(())
}
