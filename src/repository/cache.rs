use crate::package::Package;
use anyhow::{Context, Result, anyhow};
use log::debug;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::File;
use std::io;
use std::path::Path;

/// Metadata cache for [`Package`].
///
/// This cache is used to avoid expensive metadata parsing.
#[derive(Serialize, Deserialize, Default, PartialEq, Eq, Debug)]
pub struct PackageCache {
    packages: HashSet<Package>,
}

impl PackageCache {
    /// Syncs the cache by removing all packages not present in `known_packages`.
    pub fn sync(&mut self, known_packages: &HashSet<Package>) {
        self.packages.retain(|pkg| known_packages.contains(pkg));
    }

    /// Drains the cache and returns all packages.
    pub fn drain(self) -> HashSet<Package> {
        self.packages
    }

    /// Loads the package cache from the given `path`.
    ///
    /// Returns `Ok(None)` if the file doesn't exist.
    /// Returns `Err` if the cache cannot be deserialized or the file cannot be opened.
    pub fn load_from_path(path: &Path) -> Result<Option<Self>> {
        debug!("Loading package cache from {} ...", path.display());
        let reader = match File::open(path) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => Err(anyhow!(
                "unable to open package cache at {}: {err}",
                path.display()
            ))?,
        };
        Ok(Some(PackageCache::deserialize(reader)?))
    }

    /// Writes the package cache to the given `path`.
    ///
    /// Returns `Err` if the cache cannot be serialized or the file cannot be created.
    pub fn write_to_path(&self, path: &Path) -> Result<()> {
        debug!("Writing package cache to {} ...", path.display());
        let writer = File::create(path)
            .with_context(|| anyhow!("unable to create package cache '{}'", path.display()))?;
        self.serialize(writer)
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

impl FromIterator<Package> for PackageCache {
    fn from_iter<T: IntoIterator<Item = Package>>(iter: T) -> Self {
        Self {
            packages: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::metadata::PackageMetadata;
    use crate::package::version::PackageVersion;
    use std::collections::HashMap;
    use std::io::{Cursor, Seek, SeekFrom};

    impl PackageCache {
        fn insert(&mut self, package: Package) {
            self.packages.insert(package);
        }
    }

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

        let mut package = Package::new(
            "app-editors",
            "vim",
            PackageVersion::new("1.0.0", None, None).unwrap(),
            "gentoo",
        )
        .unwrap();
        let metadata = PackageMetadata::from_map(data, String::new()).unwrap();
        package.attach_metadata(metadata);

        let mut cache = PackageCache::default();
        cache.insert(package);

        let mut cursor = Cursor::new(Vec::new());
        cache.serialize(&mut cursor).unwrap();

        cursor.seek(SeekFrom::Start(0)).unwrap();
        assert_eq!(PackageCache::deserialize(cursor).unwrap(), cache);
    }
}
