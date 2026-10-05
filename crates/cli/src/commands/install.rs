use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, bail};
use germ_core::SysConf;
use germ_core::conf::portage::PortageConf;
use germ_core::package::PackageView;
use germ_core::policy::PackagePolicy;
use germ_core::policy::pkgmask::PackageMasks;
use germ_core::repository::RepoSet;
use germ_core::resolver::{ExecutionPlan, PackageOperation, PackageProvider, Resolver};
use germ_core::useflag::{EffectiveUse, UseExpandConfig};
use germ_core::vdb::Vdb;
use germ_pms::{Atom, UseFlag};

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

    print_plan(outcome.plan(), conf.use_expand_config());
    println!("Total operations: {}", outcome.plan().operations().len());
    println!("Rejected candidates: {}", outcome.rejected().len());

    Ok(())
}

fn changed_use<'a>(old: &'a EffectiveUse, new: &'a EffectiveUse) -> Vec<(&'a UseFlag, bool)> {
    let enabled = new
        .enabled()
        .difference(old.enabled())
        .map(|flag| (flag, true));
    let disabled = old
        .enabled()
        .difference(new.enabled())
        .map(|flag| (flag, false));
    let mut changes = enabled.chain(disabled).collect::<Vec<_>>();
    changes.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(b.0)));
    changes
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
        .collect::<Vec<_>>()
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
                    .iter()
                    .map(|flag| (flag, true)).collect::<Vec<_>>();
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
