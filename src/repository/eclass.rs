use crate::utils;
use anyhow::{Result, anyhow};
use lazy_static::lazy_static;
use log::trace;
use regex::Regex;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

lazy_static! {
    /// Regex to validate eclass names according to PMS 3.1.6.
    /// NOTE: look-ahead to exclude "default" is not supported by the regex crate.
    static ref ECLASS_RE: Regex = Regex::new(r"^[A-Za-z_][a-zA-Z0-9_.-]*$").unwrap();
}

#[derive(Debug)]
pub struct Eclasses(BTreeMap<String, Eclass>);

impl Eclasses {
    /// Creates a new [`Eclasses`] collection from the given `path`.
    /// Files not ending with `.eclass` are ignored.
    pub fn from_path(path: &Path) -> Result<Self> {
        let mut eclasses = Self(BTreeMap::new());
        if !path.exists() {
            return Ok(eclasses);
        }

        for file_path in utils::list_files(path)? {
            let file_path = file_path?;
            let filename = utils::path_to_filename(&file_path)?;
            if let Some(eclass_name) = filename.strip_suffix(".eclass") {
                let eclass = Eclass::new(eclass_name.to_owned(), path.join(filename))?;
                eclasses.insert(eclass);
            }
        }
        Ok(eclasses)
    }

    pub fn insert(&mut self, eclass: Eclass) {
        self.0.insert(eclass.name.clone(), eclass);
    }

    pub fn get(&self, name: &str) -> Option<&Eclass> {
        self.0.get(name)
    }
}

/// Represents an eclass defined in PMS chapter 10.
/// TODO: parse documentation
#[derive(Debug)]
pub struct Eclass {
    pub name: String,
    pub path: PathBuf,
}

impl Eclass {
    /// Creates a new [`Eclass`] from the given `name` and `path`.
    /// The name should not contain the `.eclass` suffix.
    /// Returns `Err` if `name` is invalid.
    pub fn new(name: String, path: PathBuf) -> Result<Self> {
        trace!("Loading eclass '{name}' from '{}' ...", path.display());
        debug_assert!(
            !name.ends_with(".eclass"),
            "eclass name should not contain the .eclass suffix"
        );
        if !ECLASS_RE.is_match(&name) || name == "default" {
            return Err(anyhow!("invalid eclass name: '{name}'"));
        }
        Ok(Self { name, path })
    }
}

impl fmt::Display for Eclass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eclass_new_ok() {
        let names = vec!["apache-module", "autotools", "kernel-2", "python-utils-r1"];
        for name in names {
            let eclass = Eclass::new(name.into(), PathBuf::from("/path/to/eclass.eclass"));
            assert!(eclass.is_ok(), "Eclass name '{name}' should be valid");
        }
    }

    #[test]
    fn test_eclass_new_err() {
        let names = vec![
            "-invalid-eclass",
            ".hidden-eclass",
            "invalid eclass",
            "default",
            "",
        ];
        for name in names {
            let eclass = Eclass::new(name.into(), PathBuf::from("/path/to/eclass.eclass"));
            assert!(eclass.is_err(), "Eclass name '{name}' should be invalid");
        }
    }
}
