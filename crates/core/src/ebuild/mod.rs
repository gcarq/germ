pub mod handler;

use crate::eapi::{Eapi, EapiError};

use crate::package::cpv::CPV;
use crate::repository::Repository;

use regex::Regex;
use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
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
    pub path: &'a Path,
    pub eapi: Eapi,
    pub cpv: &'a CPV,
    pub repo: &'a Repository,
}

impl<'a> Ebuild<'a> {
    /// Creates an [`Ebuild`] from the given `path` and [`CPV`] it relates to.
    ///
    /// Returns an [`EbuildError`] if the ebuild is malformed.
    pub fn new(path: &'a Path, cpv: &'a CPV, repo: &'a Repository) -> Result<Self, EbuildError> {
        let file = File::open(path).map_err(|e| EbuildError::Io {
            path: path.to_owned(),
            source: e,
        })?;
        let reader = BufReader::with_capacity(256, file);
        for line in reader.lines() {
            let line = line.map_err(|e| EbuildError::Io {
                path: path.to_owned(),
                source: e,
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
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    #[test]
    fn test_missing_eapi() {
        let file = NamedTempFile::new().unwrap();
        fs::write(file.path(), "DESCRIPTION=missing").unwrap();

        let err = Ebuild::new(file.path(), &CPV::default(), &Repository::default()).unwrap_err();

        assert!(matches!(err, EbuildError::MissingEapi));
    }

    #[test]
    fn test_unrecognized_eapi() {
        let file = NamedTempFile::new().unwrap();
        fs::write(file.path(), "EAPI=abc").unwrap();

        let err = Ebuild::new(file.path(), &CPV::default(), &Repository::default()).unwrap_err();
        assert!(matches!(
            err,
            EbuildError::Eapi(EapiError::Unrecognized(value)) if value == "abc"
        ));
    }

    #[test]
    fn test_unsupported_eapi() {
        let file = NamedTempFile::new().unwrap();
        fs::write(file.path(), "EAPI=6").unwrap();

        let err = Ebuild::new(file.path(), &CPV::default(), &Repository::default()).unwrap_err();

        assert!(matches!(err, EbuildError::UnsupportedEapi(Eapi::Six)));
    }

    #[test]
    fn test_ebuild_io_error() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("missing.ebuild");

        let err = Ebuild::new(&path, &CPV::default(), &Repository::default()).unwrap_err();

        assert!(matches!(err, EbuildError::Io { .. }));
    }

    #[test]
    fn test_ebuild_eq() {
        let path = PathBuf::from("/dev/null");
        let ebuild1 = Ebuild {
            path: &path,
            eapi: Eapi::Eight,
            cpv: &CPV::default(),
            repo: &Repository::default(),
        };
        let ebuild2 = Ebuild {
            path: &path,
            eapi: Eapi::Eight,
            cpv: &CPV::default(),
            repo: &Repository::default(),
        };
        assert_eq!(ebuild1, ebuild2);
    }
}
