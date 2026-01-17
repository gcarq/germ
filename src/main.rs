use crate::conf::PortageConf;
use crate::consts::DEFAULT_USE_PORTAGE_CONF_PATH;
use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use makenv::EnvValue;
use std::path::Path;

mod conf;
mod consts;
mod linefile;
pub mod makenv;
pub mod package;
mod profile;
mod repository;
mod utils;
mod vdb;

/// Package management tool for Gentoo-like systems.
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Provides information about the system, useful for troubleshooting
    #[arg(long)]
    info: bool,

    /// Increases verbosity
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let conf = PortageConf::new(Path::new(DEFAULT_USE_PORTAGE_CONF_PATH))?;

    if args.info {
        info(&conf);
    } else {
        println!("{}", "Masked packages:".blue().bold());
        for atom in conf.mask_manager.mask.iter() {
            println!("{atom}");
        }
        println!("\n{}", "Unmasked packages:".blue().bold());
        for atom in conf.mask_manager.unmask.iter() {
            println!("{atom}");
        }
    }

    Ok(())

    // let vdb =
    //     Vdb::from_path(PathBuf::from_str("/var/db/pkg")?).with_context(|| "Unable to build VDB")?;
    // show_updates(repo, vdb);
    // Ok(())
}

/// Prints information about the current portage configuration.
fn info(conf: &PortageConf) {
    println!("Repositories:\n\n{}", conf.repos_conf);
    let mut make_env = conf.make_env.iter().collect::<Vec<(&String, &EnvValue)>>();
    make_env.sort_by_key(|(name, _)| *name);
    for (key, value) in make_env {
        println!("{key}=\"{value}\"");
    }
}
