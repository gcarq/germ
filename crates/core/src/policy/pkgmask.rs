use std::cmp::Ordering;

use crate::deps::atom::Atom;
use crate::files::PackageEntries;
use crate::files::entry::Entry;
use crate::package::PackageView;
use crate::types::FxHashMap;
use crate::utils::Inherit;
use log::debug;

/// Simple DTO used to build [`PackageMasks`] from repo entries.
#[derive(Default)]
pub struct RepositorySource {
    pub mask: PackageEntries,
    pub unmask: PackageEntries,
}

/// Simple DTO used to build [`PackageMasks`] from user config entries.
#[derive(Default)]
pub struct PortageSource {
    pub profile_mask: PackageEntries,
    pub profile_unmask: PackageEntries,
    pub local_mask: PackageEntries,
    pub local_unmask: PackageEntries,
}

/// Immutable runtime policy that determines whether a package is masked.
pub struct PackageMasks {
    mask: FxHashMap<Box<str>, Vec<Entry<Atom>>>,
    unmask: FxHashMap<Box<str>, Vec<Entry<Atom>>>,
}

impl PackageMasks {
    /// Builds [`PackageMasks`] from repository and portage source records.
    pub fn new(repository: RepositorySource, portage: PortageSource) -> anyhow::Result<Self> {
        let mut mask = portage.profile_mask;
        let mut unmask = portage.profile_unmask;
        mask.inherit_from(&repository.mask)?;
        unmask.inherit_from(&repository.unmask)?;
        mask.inherit_from(&portage.local_mask)?;
        unmask.inherit_from(&portage.local_unmask)?;

        let policy = Self {
            mask: Self::map_from_entries(mask),
            unmask: Self::map_from_entries(unmask),
        };
        debug!(
            "Initialized MaskManager with {} masks and {} unmasks",
            policy.mask.len(),
            policy.unmask.len()
        );
        Ok(policy)
    }

    /// Checks if the given `package` is masked.
    pub fn is_masked<P: PackageView>(&self, package: &P) -> bool {
        match Self::find_match(package, &self.mask) {
            Some(mask) => match Self::find_match(package, &self.unmask) {
                Some(unmask) => match mask.prec.cmp(&unmask.prec) {
                    Ordering::Less | Ordering::Equal => !unmask.op.as_bool(),
                    Ordering::Greater => mask.op.as_bool(),
                },
                None => true,
            },
            None => false,
        }
    }

    fn find_match<'a, P: PackageView>(
        package: &P,
        entries: &'a FxHashMap<Box<str>, Vec<Entry<Atom>>>,
    ) -> Option<&'a Entry<Atom>> {
        let atoms = entries.get(&*package.qualified_name())?;
        atoms.iter().filter(|atom| package.matches_atom(atom)).max()
    }

    fn map_from_entries(entries: PackageEntries) -> FxHashMap<Box<str>, Vec<Entry<Atom>>> {
        let mut map = FxHashMap::default();
        for atom in entries.into_iter() {
            map.entry(atom.qualified_name().into())
                .or_insert_with(|| Vec::with_capacity(1))
                .push(atom);
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::entry::Precedence;
    use crate::package::Package;
    use crate::package::metadata::PackageMetadata;
    use crate::repository::RepoName;
    use crate::test_support::cpv;

    #[test]
    fn test_is_masked() -> anyhow::Result<()> {
        let masks = PackageMasks::new(
            RepositorySource::default(),
            PortageSource {
                local_mask: PackageEntries::from_string(
                    "dev-lang/rust
                        app-editors/vim"
                        .into(),
                    Precedence::User,
                )?,
                local_unmask: PackageEntries::from_string(
                    "=dev-lang/rust-1.50*".into(),
                    Precedence::User,
                )?,
                ..Default::default()
            },
        )?;

        let repo = RepoName::new("gentoo")?;
        let rust_unmasked = Package::new(
            cpv("dev-lang", "rust", "1.50-r2"),
            repo.clone(),
            PackageMetadata::default(),
        );
        let rust_masked = Package::new(
            cpv("dev-lang", "rust", "1.60-r1"),
            repo.clone(),
            PackageMetadata::default(),
        );
        let vim = Package::new(
            cpv("app-editors", "vim", "8.2"),
            repo.clone(),
            PackageMetadata::default(),
        );
        let nano = Package::new(
            cpv("app-editors", "nano", "5.0"),
            repo,
            PackageMetadata::default(),
        );

        assert!(!masks.is_masked(&rust_unmasked));
        assert!(masks.is_masked(&rust_masked));
        assert!(masks.is_masked(&vim));
        assert!(!masks.is_masked(&nano));
        Ok(())
    }
}
