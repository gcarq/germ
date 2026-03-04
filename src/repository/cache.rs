use crate::ebuild::metadata::EbuildMetadata;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

/// Metadata cache for [`Ebuild`].
///
/// This maps the absolute path of an ebuild to its metadata.
/// This cache is used to avoid reparsing ebuild files, which is expensive.
#[derive(Serialize, Deserialize, Default, PartialEq, Eq, Debug)]
pub struct MetadataCache {
    ebuilds: HashMap<PathBuf, EbuildMetadata>,
}

impl MetadataCache {
    /// Inserts the given `metadata` for the ebuild at `path` into this cache.
    pub fn insert(&mut self, path: PathBuf, metadata: EbuildMetadata) {
        self.ebuilds.insert(path, metadata);
    }

    /// Extends this cache with the entries from `ebuilds`.
    pub fn extend(&mut self, ebuilds: HashMap<PathBuf, EbuildMetadata>) {
        self.ebuilds.extend(ebuilds);
    }

    /// Deserializes the metadata cache from `reader`.
    ///
    /// Returns `Err` if the cache cannot be deserialized.
    pub fn deserialize<R>(reader: R) -> Result<Self>
    where
        R: io::Read,
    {
        ciborium::from_reader::<Self, R>(reader)
            .with_context(|| "unable to deserialize metadata cache")
    }

    /// Serializes this cache to `writer`.
    ///
    /// Returns `Err` if the cache cannot be serialized.
    pub fn serialize<W>(&self, writer: W) -> Result<()>
    where
        W: io::Write,
    {
        ciborium::into_writer(&self, writer).with_context(|| "unable to serialize metadata cache")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Seek, SeekFrom};

    #[test]
    fn test_metadata_cache_serialization() {
        let data = [
            "DEPEND=",
            "RDEPEND=dev-lang/python:3.11",
            "SLOT=0",
            "SRC_URI=https://localhost/",
            "RESTRICT=",
            "HOMEPAGE=https://localhost",
            "LICENSE=GPL-3",
            "DESCRIPTION=Example python package",
            "KEYWORDS=amd64 x86",
            "INHERITED= toolchain-funcs bash-completion-r1 eapi9-ver edo linux-info systemd",
            "IUSE=examples ipv6",
            "REQUIRED_USE=^^ ( python_single_target_python3_11 )",
            "PDEPEND=",
            "BDEPEND=",
            "EAPI=8",
            "PROPERTIES=",
            "DEFINED_PHASES=",
            "IDEPEND=",
            "INHERIT= bash-completion-r1 eapi9-ver edo linux-info systemd",
        ]
        .iter()
        .filter_map(|d| d.split_once('='))
        .collect::<HashMap<_, _>>();

        let metadata = EbuildMetadata::from_map(data, String::new()).unwrap();
        let mut cache = MetadataCache::default();
        cache.insert(PathBuf::from("/dev/null"), metadata);

        let mut cursor = Cursor::new(Vec::new());
        cache.serialize(&mut cursor).unwrap();

        cursor.seek(SeekFrom::Start(0)).unwrap();
        let cache2 = MetadataCache::deserialize(cursor).unwrap();
        assert_eq!(cache, cache2);
    }
}
