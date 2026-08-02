use anyhow::{Context, Result, anyhow};
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
pub struct PackageRevision {
    /// Source value parsed from ebuild file name
    source: Option<Box<str>>,
    /// Effective revision used for comparison
    effective: u64,
}

impl PackageRevision {
    pub fn new(revision: Option<&str>) -> Result<Self> {
        let effective = revision
            .map(|rev| {
                rev.parse::<u64>()
                    .with_context(|| anyhow!("revision must be a valid u64, got '{rev}'"))
            })
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            source: revision.map(Into::into),
            effective,
        })
    }

    /// Returns the effective revision, defaulting to zero when omitted.
    pub const fn effective(&self) -> u64 {
        self.effective
    }

    /// Returns the source revision, or `None` if omitted.
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
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
        self.effective().cmp(&other.effective())
    }
}

impl PartialOrd for PackageRevision {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for PackageRevision {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.effective().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_package_revision_semantics() {
        let implicit = PackageRevision::new(None).unwrap();
        let explicit_zero = PackageRevision::new(Some("0")).unwrap();
        let padded = PackageRevision::new(Some("03")).unwrap();
        let canonical = PackageRevision::new(Some("3")).unwrap();
        let mut revisions = HashSet::new();

        revisions.insert(implicit.clone());
        revisions.insert(explicit_zero.clone());
        revisions.insert(padded.clone());
        revisions.insert(canonical.clone());

        assert_eq!(implicit, explicit_zero);
        assert_eq!(padded, canonical);
        assert!(implicit < canonical);
        assert_eq!(implicit.effective(), 0);
        assert_eq!(implicit.source(), None);
        assert_eq!(explicit_zero.source(), Some("0"));
        assert_eq!(padded.effective(), 3);
        assert_eq!(padded.source(), Some("03"));
        assert_eq!(revisions.len(), 2);
    }
}
