use crate::conf::PortageConf;
use crate::consts::DEFAULT_USE_PORTAGE_CONF_PATH;
use crate::deps::Atom;
use crate::makenv::MakeEnv;
use crate::repository::manager::RepoManager;
use crate::vdb::Vdb;
use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use fern::colors::{Color, ColoredLevelConfig};
use log::error;
use makenv::EnvValue;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::LazyLock;

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

/// Colors for log levels.
static COLORS: LazyLock<ColoredLevelConfig> = LazyLock::new(|| {
    ColoredLevelConfig::new()
        .error(Color::Red)
        .warn(Color::Yellow)
        .info(Color::Green)
        .debug(Color::Cyan)
        .trace(Color::BrightCyan)
});

/// Package management tool for Gentoo-like systems.
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Increase verbosity
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

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

    /// Generate metadata cache for ebuild repositories
    Gencache {
        #[arg(value_name = "repo")]
        repo: Option<String>,

        #[arg(short, long)]
        force: bool,
    },

    /// Sync the repositories
    Sync,
}

fn main() {
    let args = Args::parse();
    let log_level = match args.verbose {
        0 => log::LevelFilter::Info,
        1 => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
    };
    setup_logger(log_level).expect("unable to setup logger");

    match run(args) {
        Ok(()) => (),
        Err(e) => error!("{e:#}"),
    }
}

/// Main application logic is here.
fn run(args: Args) -> Result<()> {
    let config_path = Path::new(DEFAULT_USE_PORTAGE_CONF_PATH);

    let mut repo_manager = RepoManager::new(&config_path.join("repos.conf"))
        .with_context(|| "unable to process repos.conf")?;
    let conf = PortageConf::new(Path::new(DEFAULT_USE_PORTAGE_CONF_PATH), &repo_manager)?;

    match args.command {
        Some(Command::Info { atom }) => info(atom, &repo_manager, &conf.make_env)?,
        Some(Command::Install { atom }) => install(atom, &repo_manager, &conf.make_env)?,
        Some(Command::Gencache { repo, force }) => {
            gencache(repo, &mut repo_manager, &conf.make_env, force)?;
        }
        Some(Command::Sync) => sync(&repo_manager)?,
        None => {}
    }

    Ok(())
}

/// Prints information about the current portage `conf`.
fn info(atom: Option<Atom>, repo_manager: &RepoManager, make_env: &MakeEnv) -> Result<()> {
    println!("Repositories:\n\n{repo_manager}");
    let mut make_env = make_env.iter().collect::<Vec<(&String, &EnvValue)>>();
    make_env.sort_by_key(|(name, _)| *name);
    for (key, value) in make_env {
        println!("{key}=\"{value}\"");
    }

    if let Some(atom) = atom {
        let vdb = Vdb::from_path(PathBuf::from_str("/var/db/pkg")?)
            .with_context(|| "unable to build VDB")?;

        println!("\nInstalled packages matching '{atom}':\n");
        for pkg in vdb.packages.iter().filter(|pkg| atom.matches(pkg)) {
            println!("{pkg}");
        }
    }

    Ok(())
}

/// Installs the best matching package for the given `atom`.
/// TODO: this is just a placeholder for now.
fn install(atom: Atom, repo_manager: &RepoManager, make_env: &MakeEnv) -> Result<()> {
    let pkg = repo_manager
        .repos
        .values()
        .find_map(|repo| repo.find_packages(&atom).first().map(|p| (*p).clone()));

    let Some(pkg) = pkg else {
        return Err(anyhow!("no matching package found for atom '{atom}'"));
    };
    let ebuild = repo_manager.resolve_ebuild(&pkg)?;
    let metadata = ebuild.generate_metadata(make_env)?;
    println!("{metadata}");

    Ok(())
}

/// Generates metadata cache for repositories.
fn gencache(
    repo_name: Option<String>,
    repo_manager: &mut RepoManager,
    make_env: &MakeEnv,
    force: bool,
) -> Result<()> {
    if let Some(repo) = repo_name {
        let repo = repo_manager
            .repos
            .get_mut(&repo)
            .ok_or_else(|| anyhow!("repository '{repo}' doesn't exist"))?;
        return repo.generate_metadata(make_env, force);
    }

    for repo in repo_manager.repos.values_mut() {
        repo.generate_metadata(make_env, force)?;
    }
    Ok(())
}

/// Syncs all repositories.
fn sync(repo_manager: &RepoManager) -> Result<()> {
    for repo in repo_manager.repos.values() {
        match repo.sync() {
            Ok(()) => (),
            Err(e) => error!("failed to sync repository '{}'\n\t{e}", repo.name),
        }
    }
    Ok(())
}

/// Sets up application logger with the given `log_level`.
fn setup_logger(log_level: log::LevelFilter) -> Result<()> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            let format = match record.level() {
                log::Level::Trace | log::Level::Debug => {
                    format_args!(
                        "[{}] {} - {message}",
                        COLORS.color(record.level()),
                        record.target()
                    )
                }
                _ => format_args!("[{}] {message}", COLORS.color(record.level())),
            };
            out.finish(format);
        })
        .level(log_level)
        .chain(std::io::stdout())
        .apply()?;
    Ok(())
}
