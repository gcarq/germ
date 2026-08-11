use super::{PackageVersion, base::NumberComponent, numeric::NumericComponent};
use std::iter;

/// Represents a single component of a the complete version,
/// this is necessary for correct atom wildcard matching.
#[derive(Clone, Copy, PartialEq)]
enum VersionComponent<'a> {
    Number(&'a NumberComponent),
    Letter(char),
    SuffixName(&'static str),
    SuffixNumber(&'a NumericComponent),
    Revision(&'a NumericComponent),
}

impl VersionComponent<'_> {
    /// Checks if the `candidate` version component is considered omitted for`self`.
    ///
    /// This is necessary because an omitted suffix number or revision equals a zero value.
    #[allow(clippy::match_like_matches_macro)]
    fn matches_omitted(self, candidate: Option<VersionComponent<'_>>) -> bool {
        match self {
            Self::SuffixNumber(value) if value.is_zero() => match candidate {
                Some(VersionComponent::SuffixName(_)) => true,
                Some(VersionComponent::Revision(_)) => true,
                None => true,
                _ => false,
            },
            Self::Revision(value) if value.is_zero() => candidate.is_none(),
            _ => false,
        }
    }
}

/// Checks if the `candidate` is a prefix of the given `atom` version.
pub fn matches_wildcard(atom: &PackageVersion, candidate: &PackageVersion) -> bool {
    let mut atom_comps = components(atom);
    let mut candiate_comps = components(candidate).peekable();

    loop {
        let Some(atom_comp) = atom_comps.next() else {
            return true;
        };
        let candidate_comp = candiate_comps.peek().copied();
        match candidate_comp {
            Some(candidate_comp) if atom_comp == candidate_comp => {
                candiate_comps.next();
            }
            Some(_) | None if atom_comp.matches_omitted(candidate_comp) => {}
            _ => return false,
        }
    }
}

/// Returns an [`VersionComponent`] iterator over all version components.
fn components(version: &PackageVersion) -> impl Iterator<Item = VersionComponent<'_>> {
    let components = version.number.components().map(VersionComponent::Number);
    let letter = version
        .number
        .letter()
        .into_iter()
        .map(VersionComponent::Letter);
    let suffixes = version.suffixes.iter().flat_map(|suffix| {
        iter::once(VersionComponent::SuffixName(suffix.name())).chain(
            suffix
                .number()
                .into_iter()
                .map(VersionComponent::SuffixNumber),
        )
    });
    let revision = version
        .revision
        .number()
        .into_iter()
        .map(VersionComponent::Revision);

    components.chain(letter).chain(suffixes).chain(revision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deps::atom::Atom;

    #[test]
    fn test_version_wildcard_matching() {
        let tests = [
            // suffix name and candidate extensions
            ("=cat/pkg-1_pre*", "1_pre", true),
            ("=cat/pkg-1_pre*", "1_pre1", true),
            ("=cat/pkg-1_pre*", "1_pre10", true),
            ("=cat/pkg-1_pre*", "1_rc1", false),
            ("=cat/pkg-1_pre*", "1", false),
            ("=cat/pkg-1_alpha*", "1_beta", false),
            // explicit suffix integers and component boundaries
            ("=cat/pkg-1_pre1*", "1_pre1", true),
            ("=cat/pkg-1_pre1*", "1_pre", false),
            ("=cat/pkg-1_pre1*", "1_pre0", false),
            ("=cat/pkg-1_pre1*", "1_pre10", false),
            ("=cat/pkg-1_pre0*", "1_pre", true),
            ("=cat/pkg-1_pre0*", "1_pre0", true),
            ("=cat/pkg-1_pre0*", "1_pre1", false),
            ("=cat/pkg-1_pre_beta*", "1_pre_beta", true),
            ("=cat/pkg-1_pre_beta*", "1_pre1_beta", false),
            ("=cat/pkg-1_pre0_beta*", "1_pre_beta", true),
            // revisions
            ("=cat/pkg-1-r1*", "1-r1", true),
            ("=cat/pkg-1-r1*", "1-r11", false),
            ("=cat/pkg-1-r1*", "1_alpha1", false),
            ("=cat/pkg-1-r0*", "1", true),
            ("=cat/pkg-1-r0*", "1-r0", true),
            ("=cat/pkg-1-r0*", "1-r1", false),
            ("=cat/pkg-1-r0*", "1_alpha", false),
            // numeric component boundaries
            ("=cat/pkg-1.1*", "1.1-r1", true),
            ("=cat/pkg-1.1*", "1.10-r1", false),
            // ordinary prefix
            ("=cat/pkg-1*", "1.1", true),
        ];

        for (atom, version, expected) in tests {
            let atom = Atom::new(atom).unwrap();
            let version = PackageVersion::try_from(version).unwrap();
            assert_eq!(version.matches_atom(&atom), expected);
        }
    }
}
