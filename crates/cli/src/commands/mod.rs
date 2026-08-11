//! This module contains all CLI subcommands that are available to the user.
mod gencache;
mod info;
mod install;

use std::sync::Arc;

use crate::Args;
use crate::commands::gencache::gencache;
use crate::commands::info::info;
use crate::commands::install::install;
use anyhow::Context;
use clap::Subcommand;
use germ_core::SysConf;
use germ_core::deps::atom::Atom;
use germ_core::repository::RepoSet;

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
        /// Recreate the metadata cache from scratch
        #[arg(short, long)]
        force: bool,

        /// Only generate cache for the given repository [default: all]
        #[arg(value_name = "repo")]
        repo: Option<String>,
    },

    /// Sync repositories
    Sync {
        /// Only sync the given repository [default: all]
        #[arg(value_name = "repo")]
        repo: Option<String>,
    },
}

pub async fn execute(args: &Args, sysconf: Arc<SysConf>) -> anyhow::Result<()> {
    match &args.command {
        Command::Info { atom } => info(atom.as_ref(), sysconf)?,
        Command::Install { atom } => install(atom, sysconf).await?,
        Command::Gencache { force, repo } => gencache(repo.as_deref(), *force, sysconf).await?,
        Command::Sync { repo } => sync(repo.as_deref(), sysconf)?,
    }
    Ok(())
}

/// Sync either all or the provided `repo`.
fn sync(repo: Option<&str>, sysconf: Arc<SysConf>) -> anyhow::Result<()> {
    Ok(RepoSet::new(sysconf)
        .with_context(|| "unable to build repo set")?
        .maybe_sync(repo)?)
}
