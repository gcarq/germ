use anyhow::{Context, Result, anyhow};
use rkyv::{Archive, Deserialize, Serialize};
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

/// Represents a package revision, which is an optional u64 value.
///
/// An explicit revision 0 is equal to `None`.
/// This distinction is necessary for generating the correct ebuild file name.
#[derive(Archive, Serialize, Deserialize, Clone, Copy, Debug, Default)]
pub struct PackageRevision(Option<u64>);

impl PackageRevision {
    pub fn new(revision: Option<&str>) -> Result<Self> {
        let value = revision
            .map(|rev| {
                rev.parse::<u64>()
                    .with_context(|| anyhow!("revision must be a valid u64, got '{rev}'"))
            })
            .transpose()?;
        Ok(Self(value))
    }

    /// Returns the effective revision, defaulting to zero when omitted.
    pub const fn effective(self) -> u64 {
        match self.0 {
            Some(revision) => revision,
            None => 0,
        }
    }

    /// Returns the explicit revision, or `None` if it is not set.
    pub const fn explicit(self) -> Option<u64> {
        self.0
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

    #[test]
    fn test_version_revision_ord() {
        let rev1 = PackageRevision::new(Some("1")).unwrap();
        let rev2 = PackageRevision::new(Some("2")).unwrap();
        let rev0 = PackageRevision::new(Some("0")).unwrap();
        let rev_none = PackageRevision::new(None).unwrap();

        assert_eq!(rev1.effective(), 1);
        assert_eq!(rev2.effective(), 2);
        assert_eq!(rev0.effective(), 0);
        assert_eq!(rev_none.effective(), 0);

        assert_eq!(rev0, rev_none);
        assert!(rev1 < rev2);
        assert!(rev2 > rev1);
    }
}
