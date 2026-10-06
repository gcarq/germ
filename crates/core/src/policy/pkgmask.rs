use germ_pms::Atom;
use log::debug;

use super::index::AtomIndex;
use crate::files::PackageEntries;
use crate::files::entry::{Operation, Precedence};
use crate::package::PackageView;
use crate::utils::Inherit;

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
    mask: AtomIndex<Precedence>,
    unmask: AtomIndex<Precedence>,
}

impl PackageMasks {
    /// Builds [`PackageMasks`] from repository and portage source records.
    pub fn new(repository: &RepositorySource, portage: PortageSource) -> anyhow::Result<Self> {
        let mut mask = portage.profile_mask;
        let mut unmask = portage.profile_unmask;
        mask.inherit_from(&repository.mask)?;
        unmask.inherit_from(&repository.unmask)?;
        mask.inherit_from(&portage.local_mask)?;
        unmask.inherit_from(&portage.local_unmask)?;

        let mask = Self::map_from_entries(mask);
        let unmask = Self::map_from_entries(unmask);
        debug!(
            "Initialized MaskManager with {} masks and {} unmasks",
            mask.len(),
            unmask.len()
        );
        Ok(Self {
            mask: AtomIndex::new(mask),
            unmask: AtomIndex::new(unmask),
        })
    }

    /// Checks if the given `package` is masked.
    pub fn is_masked<P: PackageView>(&self, package: &P) -> bool {
        match Self::max_precedence(&self.mask, package) {
            Some(mask) => match Self::max_precedence(&self.unmask, package) {
                Some(unmask) => mask > unmask,
                None => true,
            },
            None => false,
        }
    }

    /// Returns the highest precedence among the entries matching `package`.
    fn max_precedence<P: PackageView>(
        index: &AtomIndex<Precedence>,
        pkg: &P,
    ) -> Option<Precedence> {
        let mut max = None;
        index.visit_matches(pkg, |prec| {
            if max.is_none_or(|cur| *prec > cur) {
                max = Some(*prec);
            }
        });
        max
    }

    /// Converts `entries` into an ordered list of `(atom, precedence)` masks.
    ///
    /// Only entries with [`Operation::Set`] are included in the resulting list.
    fn map_from_entries(entries: PackageEntries) -> Vec<(Atom, Precedence)> {
        entries
            .into_iter()
            .filter_map(|entry| {
                matches!(entry.op, Operation::Set).then(|| {
                    let prec = entry.prec;
                    (entry.into_inner(), prec)
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::entry::Precedence;
    use crate::test_support::pkg;

    #[test]
    fn test_is_masked() -> anyhow::Result<()> {
        let masks = PackageMasks::new(
            &RepositorySource::default(),
            PortageSource {
                local_mask: PackageEntries::from_content(
                    "dev-lang/rust
                        app-editors/vim",
                    Precedence::User,
                )?,
                local_unmask: PackageEntries::from_content(
                    "=dev-lang/rust-1.50*",
                    Precedence::User,
                )?,
                ..Default::default()
            },
        )?;

        let rust_unmasked = pkg("dev-lang", "rust", "1.50-r2", &[]);
        let rust_masked = pkg("dev-lang", "rust", "1.60-r1", &[]);
        let vim = pkg("app-editors", "vim", "8.2", &[]);
        let nano = pkg("app-editors", "nano", "5.0", &[]);

        assert!(!masks.is_masked(&rust_unmasked));
        assert!(masks.is_masked(&rust_masked));
        assert!(masks.is_masked(&vim));
        assert!(!masks.is_masked(&nano));
        Ok(())
    }
}
