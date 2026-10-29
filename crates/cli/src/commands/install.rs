use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, bail};
use germ_core::SysConf;
use germ_core::conf::portage::PortageConf;
use germ_core::package_set::{PackageSets, Target};
use germ_core::policy::PackagePolicy;
use germ_core::policy::pkgmask::PackageMasks;
use germ_core::repository::RepoSet;
use germ_core::resolver::{
    ExecutionPlan, PackageOperation, PackageProvider, ResolutionOutcome, Resolver,
};
use germ_core::useflag::{EffectiveUse, UseExpandConfig};
use germ_core::vdb::Vdb;
use germ_pms::{PackageView, UseFlag};
use itertools::Itertools;
use log::{info, warn};

/// Installs the best matching package for each of the given `targets`.
/// TODO: this is just a placeholder for now.
pub async fn install(targets: &[Target], sysconf: Arc<SysConf>) -> anyhow::Result<()> {
    let reposet = RepoSet::new(sysconf.clone()).context("unable to build repo set")?;
    let conf = PortageConf::new(&reposet, &sysconf)?;
    let sets = PackageSets::new(&conf, &sysconf).context("unable to build package sets")?;
    let atoms = sets.resolve(targets).context("unable to resolve targets")?;
    let policy = PackagePolicy::new(
        conf.effective_keywords()?,
        conf.use_policy()?,
        PackageMasks::new(&reposet.package_mask_source()?, conf.package_mask_source()?)?,
    );

    let vdb = Vdb::from_path(sysconf.vdb_path()).context("unable to read VDB")?;

    info!("Creating plan for {}...", targets.iter().format(" "));
    let outcome = Resolver::new(PackageProvider::new(&reposet, vdb), policy)
        .resolve(&atoms)
        .await?;
    let expand = conf.use_expand_config();
    match outcome {
        ResolutionOutcome::Complete { plan, rejected } => {
            print_plan(&plan, expand);
            print_summary(&plan, rejected.len());
        }
        ResolutionOutcome::Partial {
            plan,
            failures,
            rejected,
        } => {
            print_plan(&plan, expand);
            for failure in failures {
                warn!("skipped target: {failure}");
            }
            print_summary(&plan, rejected.len());
        }
        ResolutionOutcome::Failed { failures, .. } => {
            let failures = failures.iter().format("\n");
            bail!("unable to produce an install plan:\n{failures}");
        }
    }

    Ok(())
}

fn changed_use<'a>(old: &'a EffectiveUse, new: &'a EffectiveUse) -> Vec<(&'a UseFlag, bool)> {
    new.changes(old)
        .sorted_by_key(|&(flag, enabled)| (enabled, flag))
        .collect()
}

fn fmt_flags<'a>(
    flags: impl Iterator<Item = &'a (&'a UseFlag, bool)>,
    expand: &UseExpandConfig,
) -> String {
    let mut groups = BTreeMap::<&str, Vec<String>>::new();
    for (flag, enabled) in flags {
        let (name, value) = match expand.split_expanded_flag(flag) {
            Some((group, value)) => (group.as_str(), value),
            None => ("USE", flag.as_str()),
        };
        let value = match enabled {
            true => value.to_owned(),
            false => format!("-{value}"),
        };
        groups.entry(name).or_default().push(value);
    }

    groups
        .into_iter()
        .map(|(name, values)| format!("{name}=\"{}\"", values.join(" ")))
        .join(" ")
}

/// Prints the execution plan.
fn print_plan(plan: &ExecutionPlan, expand: &UseExpandConfig) {
    let padding = 32;
    for operation in plan.operations() {
        match operation {
            PackageOperation::Merge(selected) => {
                let flags = selected
                    .effective_use
                    .enabled()
                    .map(|flag| (flag, true))
                    .collect::<Vec<_>>();
                println!(
                    "[N   ] {:<padding$} {} {}",
                    selected.pkg.qualified_name(),
                    selected.pkg.version(),
                    fmt_flags(flags.iter(), expand)
                );
            }
            PackageOperation::Replace(selected, installed) => {
                let changes = changed_use(installed.effective_use(), &selected.effective_use);
                println!(
                    "[ R  ] {:<padding$} {} {}",
                    selected.pkg.qualified_name(),
                    selected.pkg.version(),
                    fmt_flags(changes.iter(), expand)
                );
            }
            PackageOperation::Upgrade(selected, installed) => {
                let changes = changed_use(installed.effective_use(), &selected.effective_use);
                println!(
                    "[ U  ] {:<padding$} {} -> {} {}",
                    selected.pkg.qualified_name(),
                    installed.version(),
                    selected.pkg.version(),
                    fmt_flags(changes.iter(), expand)
                );
            }
            PackageOperation::Downgrade(selected, installed) => {
                let changes = changed_use(installed.effective_use(), &selected.effective_use);
                println!(
                    "[ D  ] {:<padding$} {} -> {} {}",
                    selected.pkg.qualified_name(),
                    installed.version(),
                    selected.pkg.version(),
                    fmt_flags(changes.iter(), expand)
                );
            }
            PackageOperation::Unmerge(installed) => {
                println!(
                    "[ -  ] {:<padding$} {}",
                    installed.qualified_name(),
                    installed.version()
                );
            }
        }
    }
}

/// Prints the summary of the execution plan.
fn print_summary(plan: &ExecutionPlan, rejected: usize) {
    println!("Total operations: {}", plan.operations().len());
    println!("Rejected candidates: {rejected}");
}
