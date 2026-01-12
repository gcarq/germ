use crate::conf::PortageConf;
use crate::r#const::DEFAULT_PORTAGE_CONF_PATH;
use crate::vdb::Vdb;
use anyhow::{Context, Result};
use colored::Colorize;
use makenv::EnvValue;
use std::path::{Path, PathBuf};
use std::str::FromStr;

mod conf;
mod r#const;
pub mod makenv;
pub mod package;
mod profile;
mod tree;
mod utils;
mod vdb;

fn main() -> Result<()> {
    let conf = PortageConf::new(Path::new(DEFAULT_PORTAGE_CONF_PATH))?;
    println!("Repositories:\n\n{}", conf.repos);
    let mut make_env = conf.make_env.iter().collect::<Vec<(&String, &EnvValue)>>();
    make_env.sort_by_key(|(name, _)| *name);
    for (key, value) in make_env {
        println!("{key}=\"{value}\"");
    }

    let vdb =
        Vdb::from_path(PathBuf::from_str("/var/db/pkg")?).with_context(|| "Unable to build VDB")?;
    //show_updates(repo, vdb);
    Ok(())
}

/*fn show_updates(repos: ReposConf, vdb: Vdb) {
    for local_pkg in vdb.packages {
        if let Some(repo_pkg) = repo.packages.get(&local_pkg)
            && repo_pkg.latest_version() != local_pkg.latest_version()
        {
            println!(
                "{} {}",
                format!("{}-{}", local_pkg, repo_pkg.latest_version()).green(),
                format!("[{}]", local_pkg.latest_version()).bold().blue(),
            );
        }
    }
}*/
