use crate::eapi::Eapi;
use crate::ebuild::handler::{EbuildPhase, EbuildPhaseHandler};
use crate::ebuild::metadata::EbuildMetadata;
use crate::makenv::MakeEnv;
use crate::package::Package;
use crate::repository::Repository;
use crate::utils;
use anyhow::{Context, Result, anyhow};
use log::debug;
use regex::Regex;
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::str::FromStr;
use std::sync::LazyLock;

pub mod handler;
pub mod metadata;

/// Regex to capture EAPI from ebuild files according to PMS 7.3.1.
/// The regex crate doesn't support backreferences, so we can't enforce matching quotes.
static PMS_EAPI_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^[ \t]*EAPI=['\"]?(?<eapi>[A-Za-z0-9+_.-]*)['\"]?[ \t]*([ \t]#.*)?$"#).unwrap()
});

/// An ebuild is associated with a package and contains the metadata and instructions
/// how to build it. See PMS 6 and 7.
#[derive(Clone, Eq, PartialEq)]
pub struct Ebuild<'a> {
    pub path: &'a Path,
    pub eapi: Eapi,
    pub pkg: &'a Package,
    pub repo: &'a Repository,
}

impl<'a> Ebuild<'a> {
    /// Creates an [`Ebuild`] from the given `path` and `pkg` it relates to.
    /// Returns an `Err` if the EAPI is not found or unsupported for ebuilds.
    pub fn new(path: &'a Path, pkg: &'a Package, repo: &'a Repository) -> Result<Self> {
        debug!(
            "Loading ebuild for '{pkg}' from path '{}' ...",
            path.display()
        );
        let file =
            File::open(path).with_context(|| anyhow!("unable to open {}", path.display()))?;
        let reader = BufReader::with_capacity(256, file);
        for line in reader.lines() {
            let line = line?;
            if let Some(caps) = PMS_EAPI_RE.captures(&line) {
                let eapi = Eapi::from_str(&caps["eapi"])?;
                if !eapi.is_supported_for_ebuilds() {
                    return Err(anyhow!("EAPI '{eapi}' is not supported for ebuilds"));
                }
                return Ok(Self {
                    path,
                    eapi,
                    pkg,
                    repo,
                });
            }
        }
        Err(anyhow!("EAPI not found in ebuild: {}", path.display()))
    }

    /// Generates metadata for `self` by running the ebuild `depend` phase.
    ///
    /// Returns an `Err` if the ebuild process fails or metadata is missing.
    pub fn generate_metadata(&self, make_env: &MakeEnv) -> Result<EbuildMetadata> {
        let mut handler = EbuildPhaseHandler::new(self, EbuildPhase::Depend, make_env);
        let data = handler
            .spawn()
            .with_context(|| "ebuild script execution failed")?;
        let data = data
            .iter()
            .filter_map(|d| d.split_once('='))
            .collect::<HashMap<_, _>>();

        let md5sum =
            utils::md5sum(self.path).with_context(|| anyhow!("failed to calculate md5sum"))?;
        EbuildMetadata::from_map(data, md5sum)
            .with_context(|| anyhow!("unable to create metadata from ebuild output"))
    }
}

impl fmt::Display for Ebuild<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path.display())
    }
}
