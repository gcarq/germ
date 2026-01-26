use crate::eapi::Eapi;
use anyhow::Result;
use lazy_static::lazy_static;
use regex::Regex;
use std::fs;
use std::path::PathBuf;

mod process;

lazy_static! {
    /// Regex to capture EAPI from ebuild files according to PMS 7.3.1.
    /// The regex crate doesn't support backreferences, so we can't enforce matching quotes.
    static ref PMS_EAPI_RE: Regex =
        Regex::new(r#"^[ \t]*EAPI=['\"]?(?<eapi>[A-Za-z0-9+_.-]*)['\"]?[ \t]*([ \t]#.*)?$"#).unwrap();
}

/// Represents an ebuild defined in PMS 6 and 7
#[derive(Eq, Debug, PartialEq)]
pub struct Ebuild {
    pub path: PathBuf,
    pub eapi: Eapi,
}

impl Ebuild {
    /// Creates an `Ebuild` instance from the given `path`
    pub fn from_path(path: PathBuf) -> Result<Self> {
        let content = fs::read_to_string(&path)?;
        for line in content.lines() {
            if let Some(caps) = PMS_EAPI_RE.captures(line) {
                let eapi = Eapi::new(&caps["eapi"])?;
                return Ok(Self { eapi, path });
            }
        }
        Ok(Self {
            eapi: Eapi::default(),
            path,
        })
    }
}
