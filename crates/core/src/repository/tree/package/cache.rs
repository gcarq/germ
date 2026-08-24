use once_cell::sync::OnceCell;
use std::path::{Path, PathBuf};
use std::{fs, io};

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
///
/// The cache is loaded lazily on first access.
#[derive(Debug)]
pub struct MetadataCache {
    path: PathBuf,
    db: OnceCell<Database>,
}

impl MetadataCache {
    /// Creates a new [`MetadataCache`] at the given `directory`.
    pub fn new(directory: &Path) -> Self {
        Self {
            path: directory.join(METADATA_CACHE_FILE),
            db: OnceCell::new(),
        }
    }

    /// Deletes and recreates the cache file.
    pub fn recreate(&mut self) -> Result<(), CacheError> {
        drop(self.db.take());
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    /// Inserts the given `entries` into the cache.
    pub fn insert_batch<'r>(
        &self,
        entries: impl IntoIterator<Item = (&'r CPV, &'r PackageMetadata)>,
    ) -> Result<(), CacheError> {
        let tx = self.db()?.begin_write().map_err(redb::Error::from)?;
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

        let tx = self.db()?.begin_read().map_err(redb::Error::from)?;
        let table = match tx.open_table(METADATA_TABLE) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(redb::Error::from(error).into()),
        };
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
        let tx = self.db()?.begin_write().map_err(redb::Error::from)?;
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

        let tx = self.db()?.begin_write().map_err(redb::Error::from)?;
        tx.open_table(METADATA_TABLE)
            .map_err(redb::Error::from)?
            .remove(key)
            .map_err(redb::Error::from)?;
        tx.commit().map_err(redb::Error::from)?;
        Ok(())
    }

    /// Compacts the underlying database to reclaim space.
    pub fn compact(&mut self) -> Result<(), CacheError> {
        // `Database::compact` requires exclusive access to the database handle.
        let mut db = match self.db.take() {
            Some(db) => db,
            None => Self::open(&self.path)?,
        };
        let result = db.compact().map_err(redb::Error::from);
        self.db = OnceCell::with_value(db);
        result?;
        Ok(())
    }

    /// Lazily opens the database and returns a reference to it.
    fn db(&self) -> Result<&Database, CacheError> {
        self.db.get_or_try_init(|| Self::open(&self.path))
    }

    /// Opens the database at the specified `path` and
    /// creating parent directories if necessary.
    fn open(path: &Path) -> Result<Database, CacheError> {
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory)?;
        }
        Ok(Database::create(path).map_err(redb::Error::from)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::cpv;

    #[test]
    fn test_metadata_cache_get_missing_table() {
        let temp = tempfile::tempdir().unwrap();

        let cache = MetadataCache::new(temp.path());
        let read_tx = cache.db().unwrap().begin_read().unwrap();
        assert_eq!(read_tx.list_tables().unwrap().count(), 0);
        drop(read_tx);

        let cpv = cpv("app-misc", "foo", "1");
        assert_eq!(cache.get(&cpv).unwrap(), None);
    }

    #[test]
    fn test_metadata_cache_persists_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let cpv = cpv("app-misc", "foo", "1");
        let metadata = PackageMetadata {
            description: "cached metadata".into(),
            ..Default::default()
        };

        let cache = MetadataCache::new(temp.path());
        cache.insert_batch([(&cpv, &metadata)]).unwrap();
        assert_eq!(cache.get(&cpv).unwrap(), Some(metadata.clone()));
        drop(cache);

        let reopened = MetadataCache::new(temp.path());
        assert_eq!(reopened.get(&cpv).unwrap(), Some(metadata));
    }

    #[test]
    fn test_metadata_cache_recreate() {
        let temp = tempfile::tempdir().unwrap();
        let cpv = cpv("app-misc", "foo", "1");

        let mut cache = MetadataCache::new(temp.path());
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
        let known = cpv("app-misc", "foo", "1");
        let unknown = cpv("app-misc", "bar", "1");

        let cache = MetadataCache::new(temp.path());
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
    fn test_metadata_cache_retain_compact() {
        let temp = tempfile::tempdir().unwrap();
        let known = cpv("app-misc", "foo", "1");
        let unknown = cpv("app-misc", "bar", "1");

        let mut cache = MetadataCache::new(temp.path());
        cache
            .insert_batch([
                (&known, &PackageMetadata::default()),
                (&unknown, &PackageMetadata::default()),
            ])
            .unwrap();
        cache.retain([&known]).unwrap();
        cache.compact().unwrap();
        drop(cache);

        let reopened = MetadataCache::new(temp.path());
        assert!(reopened.get(&known).unwrap().is_some());
        assert_eq!(reopened.get(&unknown).unwrap(), None);
    }

    #[test]
    fn test_metadata_cache_remove() {
        let temp = tempfile::tempdir().unwrap();

        let cache = MetadataCache::new(temp.path());
        let cpv = cpv("app-misc", "foo", "1");
        cache
            .insert_batch([(&cpv, &PackageMetadata::default())])
            .unwrap();
        cache.remove(&cpv).unwrap();

        assert_eq!(cache.get(&cpv).unwrap(), None);
    }

    #[test]
    fn test_metadata_cache_discards_corrupt_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let cache = MetadataCache::new(temp.path());
        let cpv = cpv("app-misc", "foo", "1");

        let tx = cache.db().unwrap().begin_write().unwrap();
        tx.open_table(METADATA_TABLE)
            .unwrap()
            .insert(cpv.fqn(), &b"corrupt metadata"[..])
            .unwrap();
        tx.commit().unwrap();

        assert_eq!(cache.get(&cpv).unwrap(), None);

        let read_tx = cache.db().unwrap().begin_read().unwrap();
        let table = read_tx.open_table(METADATA_TABLE).unwrap();
        assert!(table.get(cpv.fqn()).unwrap().is_none());
    }
}
