use std::sync::Arc;

use anyhow::{Context, bail};
use germ_core::SysConf;
use germ_core::atom::Atom;
use germ_core::conf::portage::PortageConf;
use germ_core::package::PackageView;
use germ_core::policy::{PackagePolicy, pkgmask::PackageMasks};
use germ_core::repository::RepoSet;
use germ_core::resolver::{ExecutionPlan, PackageOperation, PackageProvider, Resolver};
use germ_core::vdb::Vdb;

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

    let vdb = Vdb::from_path(sysconf.vdb_path()).context("unable to read VDB")?;
    let outcome = Resolver::new(PackageProvider::new(&reposet, vdb), policy)
        .resolve(atom)
        .await?;
    if let Some(failure) = outcome.failure() {
        bail!("unable to produce an install plan: {failure}");
    }

    print_plan(outcome.plan());
    println!("Total operations: {}", outcome.plan().operations().len());
    println!("Rejected candidates: {}", outcome.rejected().len());

    Ok(())
}

/// Prints the execution plan.
fn print_plan(plan: &ExecutionPlan) {
    for operation in plan.operations() {
        match operation {
            PackageOperation::Merge(selected) => {
                println!("[N   ] {}", selected.pkg.cpv());
            }
            PackageOperation::Replace(selected, _) => {
                println!("[ R  ] {}", selected.pkg.cpv());
            }
            PackageOperation::Upgrade(selected, installed) => {
                println!("[  U ] {} -> {}", installed.cpv(), selected.pkg.cpv());
            }
            PackageOperation::Downgrade(selected, installed) => {
                println!("[  D ] {} -> {}", installed.cpv(), selected.pkg.cpv());
            }
            PackageOperation::Unmerge(installed) => {
                println!("[   X] {}", installed.cpv());
            }
        }
    }
}
