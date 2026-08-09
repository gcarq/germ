pub mod handler;

use crate::eapi::{Eapi, EapiError};

use crate::ebuild::handler::error::MetadataGenerationError;
use crate::ebuild::handler::{EbuildPhase, EbuildPhaseHandler};
use crate::makenv::MakeEnv;
use crate::package::cpv::CPV;
use crate::package::metadata::PackageMetadata;
use crate::repository::Repository;
use crate::types::FxHashMap;

use anyhow::anyhow;
use regex::Regex;
use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::LazyLock;
use thiserror::Error;

/// Regex to capture EAPI from ebuild files according to PMS 7.3.1.
/// The regex crate doesn't support backreferences, so we can't enforce matching quotes.
static PMS_EAPI_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^[ \t]*EAPI=['"]?(?<eapi>[A-Za-z0-9+_.-]*)['"]?[ \t]*([ \t]#.*)?$"#).unwrap()
});

/// Errors returned when loading and validating an [`Ebuild`].
#[derive(Error, Debug)]
pub enum EbuildError {
    #[error("EAPI declaration not found")]
    MissingEapi,

    #[error(transparent)]
    Eapi(#[from] EapiError),

    #[error("unsupported EAPI '{0}'")]
    UnsupportedEapi(Eapi),

    #[error("unable to read ebuild {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// An ebuild is associated with a package and contains the metadata and instructions
/// how to build it. See PMS 6 and 7.
#[derive(Debug)]
pub struct Ebuild<'a> {
    pub path: PathBuf,
    pub eapi: Eapi,
    pub cpv: &'a CPV,
    pub repo: &'a Repository,
}

impl<'a> Ebuild<'a> {
    /// Creates an [`Ebuild`] from the given `path` and [`CPV`] it relates to.
    ///
    /// Returns an [`EbuildError`] if the ebuild is malformed.
    pub fn new(cpv: &'a CPV, repo: &'a Repository) -> Result<Self, EbuildError> {
        let path = repo
            .location
            .join(cpv.category())
            .join(cpv.package())
            .join(format!("{}.ebuild", cpv.pf()));

        let file = File::open(&path).map_err(|source| EbuildError::Io {
            path: path.clone(),
            source,
        })?;
        let reader = BufReader::with_capacity(256, file);

        for line in reader.lines() {
            let line = line.map_err(|source| EbuildError::Io {
                path: path.clone(),
                source,
            })?;
            if let Some(caps) = PMS_EAPI_RE.captures(&line) {
                let value = &caps["eapi"];
                let eapi = Eapi::from_str(value)?;
                if !eapi.is_supported_for_ebuilds() {
                    return Err(EbuildError::UnsupportedEapi(eapi));
                }
                return Ok(Self {
                    path,
                    eapi,
                    cpv,
                    repo,
                });
            }
        }
        Err(EbuildError::MissingEapi)
    }

    /// Generates and returns the [`PackageMetadata`] for this ebuild.
    ///
    /// Returns a [`MetadataGenerationError`] if it cannot be resolved.
    pub fn generate_metadata(&self) -> Result<PackageMetadata, MetadataGenerationError> {
        let mut handler = EbuildPhaseHandler::new(self, EbuildPhase::Depend, &MakeEnv::default())?;

        let data = handler.spawn()?;
        let data = data
            .iter()
            .map(|line| match line.split_once('=') {
                Some((key, value)) => Ok((key.trim(), value.trim())),
                None => Err(MetadataGenerationError::Internal(anyhow!(
                    "invalid metadata line: {line}"
                ))),
            })
            .collect::<Result<FxHashMap<_, _>, _>>();

        Ok(PackageMetadata::from_map(data?)?)
    }
}

impl Eq for Ebuild<'_> {}

impl PartialEq for Ebuild<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl fmt::Display for Ebuild<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path.display())
    }
}

#[cfg(test)]
mod tests {
    use crate::repository::test_support::RepoBuilder;

    use super::*;
    use crate::package::version::PackageVersion;
    use std::path::PathBuf;

    #[test]
    fn test_missing_eapi() {
        let repo = RepoBuilder::new("repo")
            .ebuild("cat", "pkg", "1", "DESCRIPTION=missing")
            .finalize()
            .unwrap();
        let cpv = CPV::new("cat", "pkg", PackageVersion::try_from("1").unwrap()).unwrap();
        let err = Ebuild::new(&cpv, &repo).unwrap_err();
        assert!(matches!(err, EbuildError::MissingEapi));
    }

    #[test]
    fn test_unrecognized_eapi() {
        let repo = RepoBuilder::new("repo")
            .ebuild("cat", "pkg", "1", "EAPI=abc")
            .finalize()
            .unwrap();
        let cpv = CPV::new("cat", "pkg", PackageVersion::try_from("1").unwrap()).unwrap();
        let err = Ebuild::new(&cpv, &repo).unwrap_err();
        assert!(matches!(
            err,
            EbuildError::Eapi(EapiError::Unrecognized(value)) if value == "abc"
        ));
    }

    #[test]
    fn test_unsupported_eapi() {
        let repo = RepoBuilder::new("repo")
            .ebuild("cat", "pkg", "1", "EAPI=6")
            .finalize()
            .unwrap();
        let cpv = CPV::new("cat", "pkg", PackageVersion::try_from("1").unwrap()).unwrap();
        let err = Ebuild::new(&cpv, &repo).unwrap_err();
        assert!(matches!(err, EbuildError::UnsupportedEapi(Eapi::Six)));
    }

    #[test]
    fn test_ebuild_io_error() {
        let repo = RepoBuilder::new("repo").finalize().unwrap();
        let err = Ebuild::new(&CPV::default(), &repo).unwrap_err();
        assert!(matches!(err, EbuildError::Io { .. }));
    }

    #[test]
    fn test_ebuild_eq() {
        let path = PathBuf::from("/dev/null");
        let ebuild1 = Ebuild {
            path: path.clone(),
            eapi: Eapi::Eight,
            cpv: &CPV::default(),
            repo: &Repository::default(),
        };
        let ebuild2 = Ebuild {
            path: path.clone(),
            eapi: Eapi::Eight,
            cpv: &CPV::default(),
            repo: &Repository::default(),
        };
        assert_eq!(ebuild1, ebuild2);
    }
}
