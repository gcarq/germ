use std::sync::Arc;

use anyhow::{Context, anyhow};
use germ_core::SysConf;
use germ_core::conf::portage::PortageConf;
use germ_core::deps::atom::Atom;
use germ_core::repository::RepoSet;
use log::{debug, warn};

use crate::utils::format_error;

/// Installs the best matching package for the given `atom`.
/// TODO: this is just a placeholder for now.
pub async fn install(atom: &Atom, sysconf: Arc<SysConf>) -> anyhow::Result<()> {
    let mut repo_set = RepoSet::new(sysconf.clone()).context("unable to build repo set")?;
    let conf = PortageConf::new(&repo_set, &sysconf)?;

    let keyword_policy = conf
        .keyword_policy()
        .context("unable to build keyword policy")?;

    for pkg in repo_set.find_packages(atom).await? {
        let candidate = match pkg {
            Ok(pkg) if keyword_policy.evaluate(&pkg) => pkg,
            Ok(pkg) => {
                debug!("skipping {pkg} due to missing keywords...");
                continue;
            }
            Err(err) => {
                warn!("{}", format_error(&anyhow!(err)));
                continue;
            }
        };
        println!("candidate: {candidate}");
    }
    Ok(())
}
