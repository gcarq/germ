use std::{
    fs, io,
    path::{Path, PathBuf},
};

use redb::{Database, ReadableDatabase, TableDefinition};
use rkyv::rancor;
use thiserror::Error;

use crate::package::{cpv::CPV, metadata::PackageMetadata};
use crate::types::FxHashSet;

const METADATA_CACHE_FILE: &str = "germ";
const METADATA_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata");

/// Errors that can occur while interacting with the metadata cache.
#[derive(Error, Debug)]
pub enum CacheError {
    #[error("file system operation failed")]
    Filesystem(#[from] io::Error),

    #[error("database operation failed")]
    Database(#[from] redb::Error),

    #[error("record serialization failed")]
    Serialization(#[from] rancor::BoxedError),
}

/// Holds cached metadata for packages in a repository tree using [`redb`].
#[derive(Debug)]
pub struct MetadataCache {
    db: Database,
    path: PathBuf,
}

impl MetadataCache {
    /// Creates a new [`MetadataCache`] at the given `directory`.
    pub fn new(directory: &Path) -> Result<Self, CacheError> {
        fs::create_dir_all(directory)?;
        let path = directory.join(METADATA_CACHE_FILE);
        let db = Self::open(&path)?;
        Ok(Self { db, path })
    }

    /// Deletes and recreates the cache file.
    pub fn recreate(&mut self) -> Result<(), CacheError> {
        fs::remove_file(&self.path)?;
        self.db = Self::open(&self.path)?;
        Ok(())
    }

    fn open(path: &Path) -> Result<Database, CacheError> {
        let db = Database::create(path).map_err(redb::Error::from)?;

        // Create the metadata table if it doesn't exist.
        let tx = db.begin_write().map_err(redb::Error::from)?;
        tx.open_table(METADATA_TABLE).map_err(redb::Error::from)?;
        tx.commit().map_err(redb::Error::from)?;
        Ok(db)
    }

    /// Inserts the given `entries` into the cache.
    pub fn insert_batch<'r>(
        &self,
        entries: impl IntoIterator<Item = (&'r CPV, &'r PackageMetadata)>,
    ) -> Result<(), CacheError> {
        let tx = self.db.begin_write().map_err(redb::Error::from)?;
        {
            let mut table = tx.open_table(METADATA_TABLE).map_err(redb::Error::from)?;
            for (cpv, metadata) in entries {
                let bytes = rkyv::to_bytes::<rancor::BoxedError>(metadata)?;
                table
                    .insert(cpv.fqn(), bytes.as_slice())
                    .map_err(redb::Error::from)?;
            }
        }
        tx.commit().map_err(redb::Error::from)?;
        Ok(())
    }

    /// Retrieves the metadata for the specified `cpv` from the cache.
    pub fn get(&self, cpv: &CPV) -> Result<Option<PackageMetadata>, CacheError> {
        let key = cpv.fqn();

        let tx = self.db.begin_read().map_err(redb::Error::from)?;
        let table = tx.open_table(METADATA_TABLE).map_err(redb::Error::from)?;
        let Some(value) = table.get(key).map_err(redb::Error::from)? else {
            return Ok(None);
        };
        drop(tx);

        let bytes = value.value();
        if let Ok(metadata) = rkyv::from_bytes::<PackageMetadata, rancor::BoxedError>(bytes) {
            Ok(Some(metadata))
        } else {
            self.remove(cpv)?;
            Ok(None)
        }
    }

    /// Retains only the metadata for the specified `cpvs`.
    pub fn retain<'r>(&self, cpvs: impl IntoIterator<Item = &'r CPV>) -> Result<(), CacheError> {
        let known = cpvs.into_iter().map(CPV::fqn).collect::<FxHashSet<_>>();
        let tx = self.db.begin_write().map_err(redb::Error::from)?;
        tx.open_table(METADATA_TABLE)
            .map_err(redb::Error::from)?
            .retain(|key, _| known.contains(key))
            .map_err(redb::Error::from)?;
        tx.commit().map_err(redb::Error::from)?;
        Ok(())
    }

    /// Removes the metadata for the specified `cpv` from the cache.
    pub fn remove(&self, cpv: &CPV) -> Result<(), CacheError> {
        let key = cpv.fqn();

        let tx = self.db.begin_write().map_err(redb::Error::from)?;
        tx.open_table(METADATA_TABLE)
            .map_err(redb::Error::from)?
            .remove(key)
            .map_err(redb::Error::from)?;
        tx.commit().map_err(redb::Error::from)?;
        Ok(())
    }

    /// Compacts the underlying database to reclaim space.
    pub fn compact(&mut self) -> Result<(), CacheError> {
        self.db.compact().map_err(redb::Error::from)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::version::PackageVersion;

    #[test]
    fn test_metadata_cache_get_missing() {
        let temp = tempfile::tempdir().unwrap();

        let cache = MetadataCache::new(temp.path()).unwrap();
        let cpv = CPV::new("app-misc", "foo", PackageVersion::try_from("1").unwrap()).unwrap();
        assert_eq!(cache.get(&cpv).unwrap(), None);
    }

    #[test]
    fn test_metadata_cache_persists_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let cpv = CPV::new("app-misc", "foo", PackageVersion::try_from("1").unwrap()).unwrap();
        let metadata = PackageMetadata {
            description: "cached metadata".into(),
            ..Default::default()
        };

        let cache = MetadataCache::new(temp.path()).unwrap();
        cache.insert_batch([(&cpv, &metadata)]).unwrap();
        assert_eq!(cache.get(&cpv).unwrap(), Some(metadata.clone()));
        drop(cache);

        let reopened = MetadataCache::new(temp.path()).unwrap();
        assert_eq!(reopened.get(&cpv).unwrap(), Some(metadata));
    }

    #[test]
    fn test_metadata_cache_recreate() {
        let temp = tempfile::tempdir().unwrap();
        let cpv = CPV::new("app-misc", "foo", PackageVersion::try_from("1").unwrap()).unwrap();

        let mut cache = MetadataCache::new(temp.path()).unwrap();
        cache
            .insert_batch([(&cpv, &PackageMetadata::default())])
            .unwrap();
        cache.recreate().unwrap();

        assert_eq!(cache.get(&cpv).unwrap(), None);
        assert!(temp.path().join(METADATA_CACHE_FILE).is_file());
    }

    #[test]
    fn test_metadata_cache_retain_removes_unknown_entries() {
        let temp = tempfile::tempdir().unwrap();
        let known = CPV::new("app-misc", "foo", PackageVersion::try_from("1").unwrap()).unwrap();
        let unknown = CPV::new("app-misc", "bar", PackageVersion::try_from("1").unwrap()).unwrap();

        let cache = MetadataCache::new(temp.path()).unwrap();
        cache
            .insert_batch([
                (&known, &PackageMetadata::default()),
                (&unknown, &PackageMetadata::default()),
            ])
            .unwrap();
        cache.retain([&known]).unwrap();
        assert!(cache.get(&known).unwrap().is_some());
        assert_eq!(cache.get(&unknown).unwrap(), None);
    }

    #[test]
    fn test_metadata_cache_remove() {
        let temp = tempfile::tempdir().unwrap();

        let cache = MetadataCache::new(temp.path()).unwrap();
        let cpv = CPV::new("app-misc", "foo", PackageVersion::try_from("1").unwrap()).unwrap();
        cache
            .insert_batch([(&cpv, &PackageMetadata::default())])
            .unwrap();
        cache.remove(&cpv).unwrap();

        assert_eq!(cache.get(&cpv).unwrap(), None);
    }

    #[test]
    fn test_metadata_cache_discards_corrupt_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let cache = MetadataCache::new(temp.path()).unwrap();
        let cpv = CPV::new("app-misc", "foo", PackageVersion::try_from("1").unwrap()).unwrap();

        let tx = cache.db.begin_write().unwrap();
        tx.open_table(METADATA_TABLE)
            .unwrap()
            .insert(cpv.fqn(), &b"corrupt metadata"[..])
            .unwrap();
        tx.commit().unwrap();

        assert_eq!(cache.get(&cpv).unwrap(), None);

        let read_tx = cache.db.begin_read().unwrap();
        let table = read_tx.open_table(METADATA_TABLE).unwrap();
        assert!(table.get(cpv.fqn()).unwrap().is_none());
    }
}
