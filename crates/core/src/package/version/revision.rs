use super::numeric::NumericComponent;
use rkyv::{Archive, Deserialize, Serialize};
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

/// Represents a package revision.
///
/// Revisions with different source spellings can compare equally,
/// and an explicit revision 0 is equal to an omitted revision.
///
/// For example `-r0302 == -r302`, `-r0` == None`.
#[derive(Archive, Serialize, Deserialize, Clone, Debug, Default)]
pub struct PackageRevision(Option<NumericComponent>);

impl PackageRevision {
    pub fn new(revision: Option<&str>) -> anyhow::Result<Self> {
        Ok(Self(revision.map(NumericComponent::new).transpose()?))
    }

    pub fn as_str(&self) -> Option<&str> {
        self.0.as_ref().map(NumericComponent::as_str)
    }

    pub const fn number(&self) -> Option<&NumericComponent> {
        self.0.as_ref()
    }
}

impl PartialEq for PackageRevision {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for PackageRevision {}

impl Ord for PackageRevision {
    fn cmp(&self, other: &Self) -> Ordering {
        match (&self.0, &other.0) {
            (None, None) => Ordering::Equal,
            (None, Some(right)) => match right.is_zero() {
                true => Ordering::Equal,
                false => Ordering::Less,
            },
            (Some(left), None) => match left.is_zero() {
                true => Ordering::Equal,
                false => Ordering::Greater,
            },
            (Some(left), Some(right)) => left.cmp(right),
        }
    }
}

impl PartialOrd for PackageRevision {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for PackageRevision {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0
            .as_ref()
            .map(NumericComponent::normalized)
            .unwrap_or_default()
            .hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::assert_eq_hash;

    #[test]
    fn test_package_revision_equality() {
        let implicit = PackageRevision::new(None).unwrap();
        let explicit_zero = PackageRevision::new(Some("0")).unwrap();
        let padded = PackageRevision::new(Some("03")).unwrap();
        let canonical = PackageRevision::new(Some("3")).unwrap();
        let r9999 = PackageRevision::new(Some("999999999999999999999999999999999")).unwrap();

        assert_eq_hash(&implicit, &explicit_zero);
        assert_eq_hash(&padded, &canonical);
        assert!(implicit < canonical);
        assert!(canonical < r9999);
        assert_eq!(implicit.as_str(), None);
        assert_eq!(explicit_zero.as_str(), Some("0"));
        assert_eq!(padded.as_str(), Some("03"));
        assert_eq!(r9999.as_str(), Some("999999999999999999999999999999999"));
    }
}
