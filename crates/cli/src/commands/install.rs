use std::sync::Arc;

use anyhow::{Context, anyhow};
use germ_core::conf::portage::PortageConf;
use germ_core::deps::atom::Atom;
use germ_core::policy::pkgmask::PackageMasks;
use germ_core::repository::RepoSet;
use germ_core::{SysConf, policy::PackagePolicy};
use log::{debug, warn};

use crate::utils::format_error;

/// Installs the best matching package for the given `atom`.
/// TODO: this is just a placeholder for now.
pub async fn install(atom: &Atom, sysconf: Arc<SysConf>) -> anyhow::Result<()> {
    let mut reposet = RepoSet::new(sysconf.clone()).context("unable to build repo set")?;
    let conf = PortageConf::new(&reposet, &sysconf)?;
    let policy = PackagePolicy::new(
        conf.effective_keywords()?,
        conf.use_policy()?,
        PackageMasks::new(&reposet.package_mask_source()?, conf.package_mask_source()?)?,
    );

    for pkg in reposet.find_packages(atom).await? {
        let pkg = match pkg {
            Ok(pkg) => pkg,
            Err(err) => {
                warn!("{}", format_error(&anyhow!(err)));
                continue;
            }
        };

        match policy.evaluate(&pkg) {
            Ok(true) => {
                println!("candidate: {pkg}");
            }
            Ok(false) => {
                debug!("skipping {pkg} due to policy");
            }
            Err(err) => {
                warn!("failed to evaluate {pkg}: {}", format_error(&anyhow!(err)));
            }
        }
    }

    Ok(())
}
