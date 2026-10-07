//! This module contains all CLI subcommands that are available to the user.
mod gencache;
mod info;
mod install;

use std::sync::Arc;

use anyhow::Context;
use clap::Subcommand;
use germ_core::SysConf;
use germ_core::repository::RepoSet;
use germ_pms::Atom;

use crate::Args;
use crate::commands::gencache::gencache;
use crate::commands::info::info;
use crate::commands::install::install;

#[derive(Subcommand)]
pub enum Command {
    /// Provides information about the system, useful for troubleshooting
    Info {
        /// Package atom e.g. dev-lang/rust
        #[arg(value_name = "atom")]
        atom: Option<Atom>,
    },
    /// Install packages
    Install {
        /// One or more package atoms to install, e.g. dev-lang/rust
        #[arg(value_name = "atom", required = true)]
        atoms: Vec<Atom>,
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
        Command::Info { atom } => info(atom.as_ref(), &sysconf)?,
        Command::Install { atoms } => install(atoms, sysconf).await?,
        Command::Gencache { force, repo } => gencache(repo.as_deref(), *force, sysconf).await?,
        Command::Sync { repo } => sync(repo.as_deref(), sysconf)?,
    }
    Ok(())
}

/// Sync either all or the provided `repo`.
fn sync(repo: Option<&str>, sysconf: Arc<SysConf>) -> anyhow::Result<()> {
    Ok(RepoSet::new(sysconf)
        .context("unable to build repo set")?
        .maybe_sync(repo)?)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::Args;

    #[test]
    fn test_install_requires_atom() {
        let result = Args::try_parse_from(["germ", "install"]);
        assert!(result.is_err());
    }
}
