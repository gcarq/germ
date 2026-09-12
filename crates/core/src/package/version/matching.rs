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
    #[expect(clippy::match_like_matches_macro)]
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
    let mut wildcard = components(atom);
    let mut candidate = components(candidate).peekable();

    loop {
        let Some(wc_comp) = wildcard.next() else {
            return true;
        };
        let cand_comp = candidate.peek().copied();
        match cand_comp {
            Some(cand_comp) if wc_comp == cand_comp => {
                candidate.next();
            }
            Some(_) | None if wc_comp.matches_omitted(cand_comp) => {}
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

    #[test]
    fn test_version_wildcard_matching() {
        let tests = [
            // suffix name and candidate extensions
            ("1_pre", "1_pre", true),
            ("1_pre", "1_pre1", true),
            ("1_pre", "1_pre10", true),
            ("1_pre", "1_rc1", false),
            ("1_pre", "1", false),
            ("1_alpha", "1_beta", false),
            // explicit suffix integers and component boundaries
            ("1_pre1", "1_pre1", true),
            ("1_pre1", "1_pre", false),
            ("1_pre1", "1_pre0", false),
            ("1_pre1", "1_pre10", false),
            ("1_pre0", "1_pre", true),
            ("1_pre0", "1_pre0", true),
            ("1_pre0", "1_pre1", false),
            ("1_pre_beta", "1_pre_beta", true),
            ("1_pre_beta", "1_pre1_beta", false),
            ("1_pre0_beta", "1_pre_beta", true),
            // revisions
            ("1-r1", "1-r1", true),
            ("1-r1", "1-r11", false),
            ("1-r1", "1_alpha1", false),
            ("1-r0", "1", true),
            ("1-r0", "1-r0", true),
            ("1-r0", "1-r1", false),
            ("1-r0", "1_alpha", false),
            // numeric component boundaries
            ("1.1", "1.1-r1", true),
            ("1.1", "1.10-r1", false),
            // ordinary prefix
            ("1", "1.1", true),
            ("15", "15.2.1a", true),
            ("15.2", "15.2.1a", true),
            ("15.2.1", "15.2.1a", true),
            ("15.2.1a", "15.2.1a", true),
            ("15.2.1b", "15.2.1a", false),
            ("15.2.2", "15.2.1a", false),
        ];

        for (wildcard, candidate, expected) in tests {
            let wildcard = PackageVersion::try_from(wildcard).unwrap();
            let candidate = PackageVersion::try_from(candidate).unwrap();
            assert_eq!(matches_wildcard(&wildcard, &candidate), expected);
        }
    }
}
