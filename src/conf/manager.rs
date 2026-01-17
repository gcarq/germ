use crate::linefile::LineBasedFile;
use crate::package::atom::Atom;
use crate::profile::Profile;
use crate::repository::Repository;
use crate::utils::Inherit;
use anyhow::{Context, Result};
use std::collections::HashSet;

/// Holds all active package masks and should be used as the single source of truth
/// when checking if a package is masked.
#[derive(Debug)]
pub struct MaskManager {
    pub mask: HashSet<Atom>,
    pub unmask: HashSet<Atom>,
}

impl MaskManager {
    /// Builds a [`MaskManager`] by aggregating package masks and unmasks in the following order:
    /// 1. Repository
    /// 2. Profile
    /// 3. User defined
    pub fn new(
        repos: &[&Repository],
        profile: &Profile,
        user_mask: LineBasedFile,
        user_unmask: LineBasedFile,
    ) -> Result<Self> {
        let mut mask = LineBasedFile::default().inherit(&profile.package_mask);
        let mut unmask = LineBasedFile::default().inherit(&profile.package_unmask);
        for repo in repos {
            mask.inherit_from(&repo.package_mask);
            unmask.inherit_from(&repo.package_unmask);
        }
        mask.inherit_from(&user_mask);
        unmask.inherit_from(&user_unmask);

        let mask = mask
            .into_iter()
            .map(|line| Atom::new(&line))
            .collect::<Result<HashSet<Atom>>>()
            .with_context(|| "unable to collect package masks")?;
        let unmask = unmask
            .into_iter()
            .map(|line| Atom::new(&line))
            .collect::<Result<HashSet<Atom>>>()
            .with_context(|| "unable to collect package unmasks")?;
        let manager = Self { mask, unmask };
        Ok(manager)
    }

    pub fn is_masked(&self, atom: &str) -> bool {
        todo!()
    }
}
