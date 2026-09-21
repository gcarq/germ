use std::sync::Arc;

use anyhow::Context;
use germ_core::SysConf;
use germ_core::atom::Atom;
use germ_core::conf::portage::PortageConf;
use germ_core::policy::{PackagePolicy, pkgmask::PackageMasks};
use germ_core::repository::RepoSet;
use germ_core::resolver::{RepoPkgProvider, Resolver};

/// Installs the best matching package for the given `atom`.
/// TODO: this is just a placeholder for now.
pub async fn install(atom: &Atom, sysconf: Arc<SysConf>) -> anyhow::Result<()> {
    let reposet = RepoSet::new(sysconf.clone()).context("unable to build repo set")?;
    let conf = PortageConf::new(&reposet, &sysconf)?;
    let policy = PackagePolicy::new(
        conf.effective_keywords()?,
        conf.use_policy()?,
        PackageMasks::new(&reposet.package_mask_source()?, conf.package_mask_source()?)?,
    );

    match Resolver::new(RepoPkgProvider::new(&reposet, &policy))
        .resolve(atom)
        .await?
    {
        true => Ok(()),
        false => Err(anyhow::anyhow!("unable to resolve {atom}")),
    }
}
