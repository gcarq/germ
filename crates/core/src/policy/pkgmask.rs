use germ_pms::{Atom, PackageView};
use log::debug;

use super::index::AtomIndex;
use crate::files::AtomEntries;
use crate::files::entry::{LineOrigin, Operation, Precedence};
use crate::utils::Inherit;

/// Simple DTO used to build [`PackageMasks`] from repo entries.
#[derive(Default)]
pub struct RepositorySource {
    pub mask: AtomEntries,
    pub unmask: AtomEntries,
}

/// Simple DTO used to build [`PackageMasks`] from user config entries.
#[derive(Default)]
pub struct PortageSource {
    pub profile_mask: AtomEntries,
    pub profile_unmask: AtomEntries,
    pub local_mask: AtomEntries,
    pub local_unmask: AtomEntries,
}

/// A compiled mask or unmask entry with its precedence and origin.
struct MaskEntry {
    prec: Precedence,
    origin: LineOrigin,
}

/// Immutable runtime policy that determines whether a package is masked.
pub struct PackageMasks {
    mask: AtomIndex<MaskEntry>,
    unmask: AtomIndex<MaskEntry>,
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

    /// Returns the origin of the entry masking `package`.
    pub fn masking<P: PackageView>(&self, package: &P) -> Option<LineOrigin> {
        let mask = Self::strongest(&self.mask, package)?;
        match Self::strongest(&self.unmask, package) {
            Some(unmask) if unmask.prec >= mask.prec => None,
            _ => Some(mask.origin.clone()),
        }
    }

    /// Returns the strongest entry matching `package`.
    fn strongest<'a, P: PackageView>(
        index: &'a AtomIndex<MaskEntry>,
        pkg: &P,
    ) -> Option<&'a MaskEntry> {
        let mut strongest: Option<&MaskEntry> = None;
        index.visit_matches(pkg, |entry| {
            if strongest.is_none_or(|cur| entry.prec > cur.prec) {
                strongest = Some(entry);
            }
        });
        strongest
    }

    /// Converts `entries` into an ordered list of `(atom, mask entry)` masks.
    ///
    /// Only entries with [`Operation::Set`] are included in the resulting list.
    fn map_from_entries(entries: AtomEntries) -> Vec<(Atom, MaskEntry)> {
        entries
            .into_entries()
            .filter_map(|entry| {
                matches!(entry.op, Operation::Set).then(|| {
                    let prec = entry.prec;
                    let origin = entry.origin.clone();
                    (entry.into_inner(), MaskEntry { prec, origin })
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use germ_pms::test_support::pkg;

    use super::*;
    use crate::files::entry::Precedence;
    use crate::files::test_support::origin;

    #[test]
    fn test_masking() -> anyhow::Result<()> {
        let masks = PackageMasks::new(
            &RepositorySource::default(),
            PortageSource {
                local_mask: AtomEntries::from_lines(
                    ["dev-lang/rust", "app-editors/vim"],
                    Precedence::User,
                )?,
                local_unmask: AtomEntries::from_lines(
                    "=dev-lang/rust-1.50*".lines(),
                    Precedence::User,
                )?,
                ..Default::default()
            },
        )?;

        let cases = [
            ("dev-lang/rust-1.50-r2", None),
            ("dev-lang/rust-1.60-r1", Some(origin(1))),
            ("app-editors/vim-8.2", Some(origin(2))),
            ("app-editors/nano-5.0", None),
        ];
        for (fqn, expected) in cases {
            assert_eq!(masks.masking(&pkg(fqn, &[])), expected, "{fqn}");
        }

        // A user mask beats a repository unmask, and a user unmask beats a repository mask.
        let rust = pkg("dev-lang/rust-1.60", &[]);
        let cases = [
            (
                RepositorySource {
                    unmask: AtomEntries::from_lines(
                        ["app-editors/vim", "dev-lang/rust"],
                        Precedence::Repository,
                    )?,
                    ..Default::default()
                },
                PortageSource {
                    local_mask: AtomEntries::from_lines("dev-lang/rust".lines(), Precedence::User)?,
                    ..Default::default()
                },
                Some(origin(1)),
            ),
            (
                RepositorySource {
                    mask: AtomEntries::from_lines("dev-lang/rust".lines(), Precedence::Repository)?,
                    ..Default::default()
                },
                PortageSource {
                    local_unmask: AtomEntries::from_lines(
                        "dev-lang/rust".lines(),
                        Precedence::User,
                    )?,
                    ..Default::default()
                },
                None,
            ),
        ];

        for (repository, portage, expected) in cases {
            let masks = PackageMasks::new(&repository, portage)?;
            assert_eq!(masks.masking(&rust), expected);
        }
        Ok(())
    }
}
