use crate::deps::useflag::UseFlag;
use crate::package::Package;
use crate::package::cpv::CPV;
use crate::package::metadata::PackageMetadata;
use anyhow::{Context, Result};
use std::ops::Deref;
use std::path::Path;
use std::str::FromStr;
use std::{fmt, fs};

#[cfg_attr(test, derive(Default))]
pub struct InstalledPackage {
    package: Package,
    pub use_flags: Vec<UseFlag>,
}

impl InstalledPackage {
    /// Creates a new [`InstalledPackage`] from the given `CPV`.
    ///
    /// `path` is the path to the packages vdb directory where additional metadata can be queried.
    pub fn new(cpv: CPV, path: &Path) -> Result<Self> {
        let repo = fs::read_to_string(path.join("repository"))
            .with_context(|| "unable to read repo")?
            .trim()
            .into();
        let use_flags = fs::read_to_string(path.join("USE"))
            .with_context(|| "unable to read USE flags")?
            .split_whitespace()
            .map(UseFlag::from_str)
            .collect::<Result<Vec<_>>>()?;

        let metadata =
            PackageMetadata::from_vdb_path(path).with_context(|| "unable to read metadata")?;
        let package = Package::new(cpv, repo, metadata);
        Ok(Self { package, use_flags })
    }
}

impl fmt::Display for InstalledPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.package, self.package.repo)
    }
}

impl Deref for InstalledPackage {
    type Target = Package;

    fn deref(&self) -> &Self::Target {
        &self.package
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::version::PackageVersion;

    #[test]
    fn test_installed_package_fmt() {
        let pkg = InstalledPackage {
            package: Package {
                cpv: CPV::new(
                    "app-editors",
                    "vim",
                    PackageVersion::try_from("7.0.174-r1").unwrap(),
                )
                .unwrap(),
                repo: "gentoo".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(pkg.to_string(), "app-editors/vim-7.0.174-r1::gentoo");
    }
}
