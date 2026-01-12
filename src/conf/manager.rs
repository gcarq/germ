use crate::linefile::LineBasedFile;
use crate::profile::Profile;
use crate::repository::Repository;
use crate::utils::Inherit;
use std::collections::HashSet;

/// Holds all active package masks and should be used as the single source of truth
/// when checking if a package is masked.
#[derive(Debug)]
pub struct MaskManager {
    mask: HashSet<String>,
}

impl MaskManager {
    /// Builds a [`MaskManager`] by aggregating package masks from the given repositories and profile.
    pub fn new(repos: &[&Repository], profile: &Profile, user_mask: &LineBasedFile) -> Self {
        let mut mask = user_mask.inherit(&profile.package_mask);
        for repo in repos {
            mask.inherit_from(&repo.package_mask)
        }
        Self {
            mask: HashSet::from_iter(mask.into_iter()),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.mask.iter()
    }

    pub fn is_masked(&self, atom: &str) -> bool {
        todo!()
    }
}

#[derive(Debug)]
pub struct UseManager {}

impl UseManager {
    pub fn new(profile: &Profile) -> Self {
        todo!()
    }
}
