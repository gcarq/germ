use germ_pms::UseFlag;

use super::PackagePolicy;
use super::keyword::EffectiveKeywords;
use super::pkgmask::{PackageMasks, PortageSource, RepositorySource};
use super::useflag::{LocalRecords, UsePolicy};
use crate::files::PackageEntries;
use crate::files::entry::Precedence;
use crate::files::pkgfile::PackageAcceptKeywords;
use crate::keyword::KeywordSelector;
use crate::profile::ProfileUseRecords;
use crate::types::{FxHashMap, FxHashSet};

/// Builds a [`PackagePolicy`] for tests.
#[derive(Default)]
pub(crate) struct PolicyFixture {
    use_flags: Vec<UseFlag>,
    masks: Vec<String>,
}

impl PolicyFixture {
    /// Sets the globally enabled USE flags.
    pub fn with_use(mut self, flags: &[&str]) -> Self {
        self.use_flags = flags.iter().map(|flag| flag.parse().unwrap()).collect();
        self
    }

    /// Adds a package mask atom.
    pub fn with_mask(mut self, atom: impl Into<String>) -> Self {
        self.masks.push(atom.into());
        self
    }

    /// Builds and returns the policy.
    pub fn build(&self) -> anyhow::Result<PackagePolicy> {
        let keywords =
            EffectiveKeywords::new(vec![KeywordSelector::Any], PackageAcceptKeywords::default());
        let usepolicy = UsePolicy::new(
            FxHashMap::from_iter(self.use_flags.iter().cloned().map(|flag| (flag, true))),
            FxHashSet::default(),
            ProfileUseRecords::default(),
            LocalRecords::default(),
        )?;
        let pkgmasks = PackageMasks::new(
            &RepositorySource::default(),
            PortageSource {
                local_mask: PackageEntries::from_content(&self.masks.join("\n"), Precedence::User)?,
                ..Default::default()
            },
        )?;
        Ok(PackagePolicy::new(keywords, usepolicy, pkgmasks))
    }
}
