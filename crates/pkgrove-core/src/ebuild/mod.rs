pub mod handler;

use crate::eapi::Eapi;
use crate::package::cpv::CPV;
use crate::repository::Repository;
use anyhow::{Context, Result, anyhow, bail};
use log::trace;
use regex::Regex;
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::LazyLock;

/// Regex to capture EAPI from ebuild files according to PMS 7.3.1.
/// The regex crate doesn't support backreferences, so we can't enforce matching quotes.
static PMS_EAPI_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^[ \t]*EAPI=['"]?(?<eapi>[A-Za-z0-9+_.-]*)['"]?[ \t]*([ \t]#.*)?$"#).unwrap()
});

/// An ebuild is associated with a package and contains the metadata and instructions
/// how to build it. See PMS 6 and 7.
#[cfg_attr(test, derive(Debug))]
pub struct Ebuild<'a> {
    pub path: PathBuf,
    pub eapi: Eapi,
    pub cpv: &'a CPV,
    pub repo: &'a Repository,
}

impl<'a> Ebuild<'a> {
    /// Creates an [`Ebuild`] from the given `path` and [`CPV`] it relates to.
    ///
    /// Returns an `Err` if the EAPI is not found or unsupported for ebuilds.
    pub fn new(path: PathBuf, cpv: &'a CPV, repo: &'a Repository) -> Result<Self> {
        trace!("Loading ebuild '{}' for '{cpv}' ...", path.display());
        let file =
            File::open(&path).with_context(|| anyhow!("unable to open {}", path.display()))?;
        let reader = BufReader::with_capacity(256, file);
        for line in reader.lines() {
            let line = line?;
            if let Some(caps) = PMS_EAPI_RE.captures(&line) {
                let eapi = Eapi::from_str(&caps["eapi"])?;
                if !eapi.is_supported_for_ebuilds() {
                    bail!("EAPI '{eapi}' is not supported for ebuilds");
                }
                return Ok(Self {
                    path,
                    eapi,
                    cpv,
                    repo,
                });
            }
        }
        bail!("EAPI not found in ebuild: {}", path.display())
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

    #[test]
    fn test_ebuild_eq() {
        let ebuild1 = Ebuild {
            path: PathBuf::from("/dev/null"),
            eapi: Eapi::Eight,
            cpv: &CPV::default(),
            repo: &Repository::default(),
        };
        let ebuild2 = Ebuild {
            path: PathBuf::from("/dev/null"),
            eapi: Eapi::Eight,
            cpv: &CPV::default(),
            repo: &Repository::default(),
        };
        assert_eq!(ebuild1, ebuild2);
    }
}
