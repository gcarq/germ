pub mod handler;

use crate::eapi::{Eapi, EapiError};

use crate::ebuild::handler::error::MetadataGenerationError;
use crate::ebuild::handler::{EbuildPhase, EbuildPhaseHandler};
use crate::makenv::MakeEnv;
use crate::package::cpv::CPV;
use crate::package::metadata::PackageMetadata;
use crate::repository::Repository;
use crate::types::FxHashMap;
use crate::utils::is_blank_or_comment;

use anyhow::anyhow;
use fancy_regex::Regex;
use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::LazyLock;
use thiserror::Error;

/// Regex for a PMS 7.3.1 EAPI declaration.
static PMS_EAPI_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^[ \t]*EAPI=(?<quote>['\"]?)(?<value>[A-Za-z0-9+_.-]*)\k<quote>(?:[ \t]+#.*|[ \t]*)$"#,
    )
    .unwrap()
});

/// Errors returned when loading and validating an [`Ebuild`].
#[derive(Error, Debug)]
pub enum EbuildError {
    #[error(transparent)]
    Eapi(#[from] EapiError),

    #[error("unsupported EAPI '{0}'")]
    UnsupportedEapi(Eapi),

    #[error("internal ebuild error")]
    Internal(#[from] anyhow::Error),

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
pub struct Ebuild<'r> {
    pub path: PathBuf,
    pub eapi: Eapi,
    pub cpv: &'r CPV,
    pub repo: &'r Repository,
}

impl<'r> Ebuild<'r> {
    /// Creates an [`Ebuild`] from the given `path` and [`CPV`] it relates to.
    ///
    /// Returns an [`EbuildError`] if the ebuild is malformed.
    pub fn new(cpv: &'r CPV, repo: &'r Repository) -> Result<Self, EbuildError> {
        let path = repo
            .location
            .join(cpv.category())
            .join(cpv.package())
            .join(format!("{}.ebuild", cpv.pf()));

        let eapi = Self::parse_eapi(&path)?;
        if !eapi.is_supported_for_ebuilds() {
            return Err(EbuildError::UnsupportedEapi(eapi));
        }

        Ok(Self {
            path,
            eapi,
            cpv,
            repo,
        })
    }

    /// Generates and returns the [`PackageMetadata`] for this ebuild.
    ///
    /// Returns a [`MetadataGenerationError`] if it cannot be resolved.
    pub async fn generate_metadata(&self) -> Result<PackageMetadata, MetadataGenerationError> {
        let mut handler = EbuildPhaseHandler::new(self, EbuildPhase::Depend, &MakeEnv::default())?;

        let data = handler.spawn().await?;
        let data = data
            .iter()
            .map(|line| match line.split_once('=') {
                Some((key, value)) => Ok((key.trim(), value.trim())),
                None => Err(anyhow!("invalid metadata line: {line}")),
            })
            .collect::<Result<FxHashMap<_, _>, _>>();

        Ok(PackageMetadata::from_map(data?)?)
    }

    /// Parses the EAPI from the ebuild file at the given `path`.
    ///
    /// The EAPI is expected to be declared in the first non-blank and non-comment line,
    /// otherwise [`Eapi::Zero`] will be returned.
    fn parse_eapi(path: &Path) -> Result<Eapi, EbuildError> {
        let file = File::open(path).map_err(|source| EbuildError::Io {
            path: path.to_owned(),
            source,
        })?;
        let reader = BufReader::with_capacity(256, file);

        let first_line = reader
            .lines()
            .find_map(|line| match line {
                Ok(line) if !is_blank_or_comment(&line) => Some(Ok(line)),
                Ok(_) => None,
                Err(err) => Some(Err(err)),
            })
            .transpose()
            .map_err(|source| EbuildError::Io {
                path: path.to_owned(),
                source,
            })?;

        let Some(first_line) = first_line else {
            return Ok(Eapi::Zero);
        };
        match Self::parse_eapi_declaration(&first_line)? {
            Some("") | None => Ok(Eapi::Zero),
            Some(value) => Ok(Eapi::from_str(value)?),
        }
    }

    /// Parses a PMS EAPI assignment and returns its raw value.
    fn parse_eapi_declaration(line: &str) -> anyhow::Result<Option<&str>> {
        Ok(PMS_EAPI_RE
            .captures(line)?
            .and_then(|captures| captures.name("value").map(|value| value.as_str())))
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
    use crate::test_support::cpv;

    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_eapi_declarations() {
        for (line, expected) in [
            ("EAPI=7", Some("7")),
            ("  EAPI='7'", Some("7")),
            ("EAPI=\"7\" # comment", Some("7")),
            ("EAPI='7\"", None),
            ("EAPI=\"7'", None),
            ("EAPI = 7", None),
        ] {
            assert_eq!(Ebuild::parse_eapi_declaration(line).unwrap(), expected);
        }
    }

    #[test]
    fn test_ebuild_without_eapi() {
        let contents = ["", "  # comment", "DESCRIPTION=missing", "EAPI="];
        let mut builder = RepoBuilder::new("repo");
        for (index, content) in contents.iter().enumerate() {
            builder = builder.ebuild("cat", "pkg", (index + 1).to_string(), *content);
        }
        let repo = builder.finalize().unwrap();

        for (index, _) in contents.iter().enumerate() {
            let cpv = cpv("cat", "pkg", &(index + 1).to_string());
            assert!(matches!(
                Ebuild::new(&cpv, &repo).unwrap_err(),
                EbuildError::UnsupportedEapi(Eapi::Zero)
            ));
        }
    }

    #[test]
    fn test_unrecognized_eapi() {
        let repo = RepoBuilder::new("repo")
            .ebuild("cat", "pkg", "1", "EAPI=abc")
            .finalize()
            .unwrap();
        let cpv = cpv("cat", "pkg", "1");
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
        let cpv = cpv("cat", "pkg", "1");
        let err = Ebuild::new(&cpv, &repo).unwrap_err();
        assert!(matches!(err, EbuildError::UnsupportedEapi(Eapi::Six)));
    }

    #[test]
    fn test_ebuild_io_error() {
        let repo = RepoBuilder::new("repo").finalize().unwrap();
        let cpv = cpv("cat", "pkg", "1");
        let err = Ebuild::new(&cpv, &repo).unwrap_err();
        assert!(matches!(err, EbuildError::Io { .. }));
    }

    #[test]
    fn test_ebuild_eq() {
        let path = PathBuf::from("/dev/null");
        let cpv = cpv("cat", "pkg", "1");
        let repo = Repository::default();
        let ebuild1 = Ebuild {
            path: path.clone(),
            eapi: Eapi::Eight,
            cpv: &cpv,
            repo: &repo,
        };
        let ebuild2 = Ebuild {
            path: path.clone(),
            eapi: Eapi::Eight,
            cpv: &cpv,
            repo: &repo,
        };
        assert_eq!(ebuild1, ebuild2);
    }
}
