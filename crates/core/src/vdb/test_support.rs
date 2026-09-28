use tempfile::{TempDir, tempdir};

use super::Vdb;
use std::path::PathBuf;
use std::{fs, io};

/// Holds a temporary VDB filesystem fixture.
pub struct VdbFixture {
    temp_dir: TempDir,
}

impl VdbFixture {
    /// Creates an empty VDB fixture.
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            temp_dir: tempdir()?,
        })
    }

    /// Defines an installed package in the fixture.
    pub fn package<'a>(
        &'a self,
        category: &'a str,
        package: &'a str,
        version: &'a str,
    ) -> VdbPackageBuilder<'a> {
        VdbPackageBuilder {
            fixture: self,
            category,
            package,
            version,
            repo: "gentoo",
            slot: "0",
        }
    }

    /// Creates a [`Vdb`] backed by this fixture.
    pub fn vdb(&self) -> anyhow::Result<Vdb> {
        Vdb::from_path(self.temp_dir.path())
    }
}

/// Defines an installed package in a [`VdbFixture`].
pub struct VdbPackageBuilder<'a> {
    fixture: &'a VdbFixture,
    category: &'a str,
    package: &'a str,
    version: &'a str,
    repo: &'a str,
    slot: &'a str,
}

impl<'a> VdbPackageBuilder<'a> {
    /// Sets the package repository.
    pub fn repo(mut self, repo: &'a str) -> Self {
        self.repo = repo;
        self
    }

    /// Sets the package slot.
    pub fn slot(mut self, slot: &'a str) -> Self {
        self.slot = slot;
        self
    }

    /// Writes the package metadata to the fixture.
    pub fn write(self) -> io::Result<()> {
        let path = self.path();
        fs::create_dir_all(&path)?;
        fs::write(path.join("repository"), self.repo)?;
        fs::write(path.join("USE"), "")?;
        fs::write(path.join("IUSE_EFFECTIVE"), "")?;
        fs::write(path.join("EAPI"), "8")?;
        fs::write(path.join("DESCRIPTION"), "Test package")?;
        fs::write(path.join("SLOT"), self.slot)
    }

    fn path(&self) -> PathBuf {
        self.fixture
            .temp_dir
            .path()
            .join(self.category)
            .join(format!("{}-{}", self.package, self.version))
    }
}
