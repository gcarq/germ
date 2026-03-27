use crate::deps::atom::Atom;
use crate::package::Package;
use crate::package::cpv::CPV;
use crate::types::FxHashMap;
use anyhow::{Context, Result, anyhow};
use log::debug;
use rkyv::rancor;
use rkyv::with::Skip;
use rkyv::{Archive, Deserialize, Serialize};
use std::fs::File;
use std::io;
use std::ops::Deref;
use std::path::Path;

/// Holds all available packages in a repository, mapping `category/package` to [`Vec<CPV>`].
#[derive(Default)]
#[cfg_attr(test, derive(Debug))]
pub struct AvailablePackageIndex(FxHashMap<String, Vec<CPV>>);

impl AvailablePackageIndex {
    /// Inserts the given `cpv` into the index.
    pub fn insert(&mut self, cpv: CPV) {
        let entry = self.0.entry(cpv.qualified_name()).or_default();
        entry.push(cpv);
        entry.sort_unstable_by(|a, b| b.cmp(a));
    }

    pub fn insert_all(&mut self, cpvs: Vec<CPV>) {
        for cpv in cpvs {
            self.insert(cpv);
        }
    }

    /// Checks if the index contains the given `cpv`.
    pub fn contains(&self, cpv: &CPV) -> bool {
        self.0
            .get(&cpv.qualified_name())
            .is_some_and(|cpvs| cpvs.contains(cpv))
    }

    /// Returns all packages matching the given `atom`.
    pub fn find_packages(&self, atom: &Atom) -> Option<Vec<&CPV>> {
        let pkgs = self.0.get(&atom.qualified_name())?;
        let matching_pkgs = pkgs.iter().filter(|cpv| cpv.matches_atom(atom)).collect();
        Some(matching_pkgs)
    }
}

impl Deref for AvailablePackageIndex {
    type Target = FxHashMap<String, Vec<CPV>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Holds all resolved packages in a repository, mapping the fully qualified name
/// `category/package-version` to a [`Package`].
#[derive(Archive, Serialize, Deserialize, Default)]
#[cfg_attr(test, derive(Debug))]
pub struct ResolvedPackageIndex {
    index: FxHashMap<String, Package>,
    #[rkyv(with = Skip)]
    modified: bool,
}

impl ResolvedPackageIndex {
    /// Inserts a package into the index.
    pub fn insert(&mut self, package: Package) {
        self.index.insert(package.cpv.fqn().into(), package);
        self.modified = true;
    }

    /// Extends the index with the given packages.
    pub fn extend(&mut self, packages: Vec<Package>) {
        let len = self.index.len();
        self.index
            .extend(packages.into_iter().map(|pkg| (pkg.cpv.fqn().into(), pkg)));
        if self.index.len() != len {
            self.modified = true;
        }
    }

    /// Retains only the packages that are present in the given [`AvailablePackageIndex`].
    pub fn retain(&mut self, available: &AvailablePackageIndex) {
        let len = self.index.len();
        self.index.retain(|_, pkg| available.contains(&pkg.cpv));
        if self.index.len() != len {
            self.index.shrink_to_fit();
            self.modified = true;
            debug!(
                "Removed {} packages from the resolved index",
                len - self.index.len()
            );
        }
    }

    /// Loads the index from the given `path`.
    ///
    /// Returns `Ok(None)` if the file doesn't exist.
    /// Returns `Err` if the index cannot be deserialized or the file cannot be opened.
    pub fn load_from_path(path: &Path) -> Result<Option<Self>> {
        let reader = match File::open(path) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => Err(anyhow!(
                "unable to open package index at {}: {err}",
                path.display()
            ))?,
        };
        Ok(Some(Self::deserialize(reader)?))
    }

    /// Writes the index to the given `path`.
    ///
    /// If the index hasn't been modified and `force` is not set, this is a no-op.
    /// Returns `Err` if the index cannot be serialized or the file cannot be created.
    pub fn write_to_path(&self, path: &Path, force: bool) -> Result<()> {
        if !force && !self.modified {
            debug!("Index not modified, skipping write",);
            return Ok(());
        }
        let writer = File::create(path)
            .with_context(|| anyhow!("unable to write to '{}'", path.display()))?;
        self.serialize(writer)
    }

    /// Deserializes an index from `reader`.
    ///
    /// Returns `Err` if the index cannot be deserialized.
    pub fn deserialize<R>(mut reader: R) -> Result<Self>
    where
        R: io::Read,
    {
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)?;

        let index = rkyv::from_bytes::<ResolvedPackageIndex, rancor::Error>(&buf)
            .with_context(|| anyhow!("unable to deserialize"))?;
        Ok(index)
    }

    /// Serializes this index to `writer`.
    ///
    /// Returns `Err` if the index cannot be serialized.
    pub fn serialize<W>(&self, mut writer: W) -> Result<()>
    where
        W: io::Write,
    {
        let bytes = rkyv::to_bytes::<rancor::Error>(self).with_context(|| "unable to serialize")?;
        writer.write_all(&bytes)?;
        Ok(())
    }
}

impl Deref for ResolvedPackageIndex {
    type Target = FxHashMap<String, Package>;
    fn deref(&self) -> &Self::Target {
        &self.index
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::version::PackageVersion;
    use std::io::{Cursor, Seek, SeekFrom};

    #[test]
    fn test_available_package_index() {
        let mut index = AvailablePackageIndex::default();
        let python_3_13 = CPV::new(
            "dev-lang",
            "python",
            PackageVersion::new("3.13.12", None, None).unwrap(),
        )
        .unwrap();
        index.insert(python_3_13.clone());
        let python3_14 = CPV::new(
            "dev-lang",
            "python",
            PackageVersion::new("3.14.3", None, None).unwrap(),
        )
        .unwrap();
        index.insert(python3_14.clone());

        let rust = CPV::new(
            "dev-lang",
            "rust",
            PackageVersion::new("1.94.0", None, None).unwrap(),
        )
        .unwrap();
        assert!(!index.contains(&rust));
        index.insert(rust.clone());

        let packages = index
            .find_packages(&Atom::new("dev-lang/python").unwrap())
            .unwrap();
        assert_eq!(&packages, &[&python3_14, &python_3_13]);
        assert!(index.contains(&rust));
    }

    #[test]
    fn test_resolved_package_index() {
        let mut index = ResolvedPackageIndex::default();

        let cpv = CPV::new(
            "dev-lang",
            "python",
            PackageVersion::new("3.13.12", None, None).unwrap(),
        )
        .unwrap();
        let pkg = Package {
            cpv: cpv.clone(),
            ..Default::default()
        };
        assert!(!index.contains_key(cpv.fqn()));

        index.insert(pkg);
        assert!(index.contains_key(cpv.fqn()));
        assert_eq!(cpv, index.get(cpv.fqn()).unwrap().cpv);
    }

    #[test]
    fn test_resolved_package_index_serialization() {
        let mut index = ResolvedPackageIndex::default();

        let cpv = CPV::new(
            "media-libs",
            "mesa",
            PackageVersion::new("26.0.1", None, None).unwrap(),
        )
        .unwrap();
        let pkg = Package {
            cpv: cpv.clone(),
            ..Default::default()
        };
        index.insert(pkg);
        assert!(index.contains_key(cpv.fqn()));

        let mut cursor = Cursor::new(Vec::new());
        index.serialize(&mut cursor).unwrap();

        cursor.seek(SeekFrom::Start(0)).unwrap();
        let index = ResolvedPackageIndex::deserialize(&mut cursor).unwrap();
        assert!(index.contains_key(cpv.fqn()));
    }

    #[test]
    fn test_resolved_package_index_retain() {
        let mut resolved = ResolvedPackageIndex::default();

        let cpv1 = CPV::new(
            "dev-lang",
            "python",
            PackageVersion::new("3.13.12", None, None).unwrap(),
        )
        .unwrap();
        resolved.insert(Package {
            cpv: cpv1.clone(),
            ..Default::default()
        });

        let cpv2 = CPV::new(
            "dev-lang",
            "rust",
            PackageVersion::new("1.94.0", None, None).unwrap(),
        )
        .unwrap();
        resolved.insert(Package {
            cpv: cpv2.clone(),
            ..Default::default()
        });

        let mut available = AvailablePackageIndex::default();
        available.insert(cpv1.clone());

        resolved.retain(&available);
        assert!(resolved.contains_key(cpv1.fqn()));
        assert!(!resolved.contains_key(cpv2.fqn()));
    }
}
