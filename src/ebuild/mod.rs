use crate::eapi::Eapi;
use crate::package::Package;
use anyhow::{Result, anyhow};
use lazy_static::lazy_static;
use log::debug;
use regex::Regex;
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

pub mod handler;

lazy_static! {
    /// Regex to capture EAPI from ebuild files according to PMS 7.3.1.
    /// The regex crate doesn't support backreferences, so we can't enforce matching quotes.
    static ref PMS_EAPI_RE: Regex =
        Regex::new(r#"^[ \t]*EAPI=['\"]?(?<eapi>[A-Za-z0-9+_.-]*)['\"]?[ \t]*([ \t]#.*)?$"#).unwrap();
}

/// Represents an ebuild defined in PMS 6 and 7
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ebuild<'a> {
    pub path: PathBuf,
    pub eapi: Eapi,
    pub pkg: &'a Package,
}

impl<'a> Ebuild<'a> {
    /// Creates an [`Ebuild`] from the given `path` and `pkg` it relates to.
    /// Returns an `Err` if the EAPI is not found or unsupported for ebuilds.
    pub fn new(path: PathBuf, pkg: &'a Package) -> Result<Self> {
        debug!("Loading ebuild for '{pkg}' from path '{}'", path.display());
        let reader = BufReader::with_capacity(256, File::open(&path)?);
        for line in reader.lines() {
            let line = line?;
            if let Some(caps) = PMS_EAPI_RE.captures(&line) {
                let eapi = Eapi::new(&caps["eapi"])?;
                if !eapi.is_supported_for_ebuilds() {
                    return Err(anyhow!(
                        "EAPI '{}' is not supported for ebuilds",
                        eapi.version
                    ));
                }
                return Ok(Self { eapi, path, pkg });
            }
        }
        Err(anyhow!("EAPI not found in ebuild: {}", path.display()))
    }
}

impl fmt::Display for Ebuild<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path.display())
    }
}
