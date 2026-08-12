use anyhow::bail;
use fancy_regex::Regex;
use log::trace;
use rkyv::with::AsString;
use rkyv::{Archive, Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use walkdir::WalkDir;

/// Regex to validate eclass names according to PMS 3.1.6.
static ECLASS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z_][a-zA-Z0-9_.-]*$").unwrap());

/// Contains all known eclasses, including inherited eclasses,
/// and their repository lookup paths.
#[derive(Debug)]
#[cfg_attr(test, derive(Default))]
pub struct Eclasses {
    entries: BTreeMap<String, Eclass>,
    repo_paths: Vec<PathBuf>,
}

impl Eclasses {
    /// Creates an empty [`Eclasses`] collection for the given repository `path`.
    pub fn empty(path: &Path) -> Self {
        Self {
            entries: BTreeMap::default(),
            repo_paths: vec![path.to_owned()],
        }
    }

    /// Creates a new [`Eclasses`] collection from the given eclass `path`.
    /// Files not ending with `.eclass` are ignored.
    ///
    /// Returns `Err` if the parent of `path` doesn't exist.
    pub fn from_path(path: &Path) -> anyhow::Result<Self> {
        let Some(parent) = path.parent() else {
            bail!("parent {} doesn't exist", path.display());
        };
        let mut eclasses = Self {
            entries: BTreeMap::default(),
            repo_paths: vec![parent.to_owned()],
        };
        if !path.exists() {
            return Ok(eclasses);
        }

        let entries = WalkDir::new(path)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_entry(|e| e.file_type().is_file())
            .filter_map(Result::ok);

        for entry in entries {
            let Some(filename) = entry.file_name().to_str() else {
                continue;
            };
            if let Some(name) = filename.strip_suffix(".eclass") {
                eclasses.insert(Eclass::new(name.to_owned(), entry.path().to_owned())?);
            }
        }
        Ok(eclasses)
    }

    /// Returns repository paths in eclass lookup order.
    ///
    /// The first path is the repository being processed,
    /// followed by its masters in declared order.
    ///
    /// While this is an implementation detail of portage,
    /// some overlay eclasses rely on it.
    pub fn repo_paths(&self) -> impl Iterator<Item = &Path> {
        self.repo_paths.iter().map(PathBuf::as_path)
    }

    /// Inserts an [`Eclass`] into the collection.
    pub fn insert(&mut self, eclass: Eclass) {
        if let Some(path) = eclass.path.parent().and_then(Path::parent)
            && !self.repo_paths.iter().any(|known| known == path)
        {
            self.repo_paths.push(path.to_owned());
        }
        self.entries.insert(eclass.name.clone(), eclass);
    }

    /// Extends the current collection with the given `other`.
    pub fn extend(&mut self, other: &Self) {
        for path in &other.repo_paths {
            if !self.repo_paths.contains(path) {
                self.repo_paths.push(path.clone());
            }
        }
        self.entries.extend(other.entries.clone());
    }
}

impl Deref for Eclasses {
    type Target = BTreeMap<String, Eclass>;
    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

/// Represents an eclass defined in PMS chapter 10.
/// TODO: parse documentation
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Debug)]
pub struct Eclass {
    pub name: String,
    #[rkyv(with = AsString)]
    pub path: PathBuf,
}

impl Eclass {
    /// Creates a new [`Eclass`] from the given `name` and `path`.
    /// The name should not contain the `.eclass` suffix.
    /// Returns `Err` if `name` is invalid.
    pub fn new(name: String, path: PathBuf) -> anyhow::Result<Self> {
        trace!("Loading eclass '{name}' from '{}' ...", path.display());
        debug_assert!(
            !name.ends_with(".eclass"),
            "eclass name should not contain the .eclass suffix"
        );
        if !ECLASS_RE.is_match(&name)? || name == "default" {
            bail!("invalid eclass name: '{name}'");
        }
        Ok(Self { name, path })
    }
}

impl fmt::Display for Eclass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
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
            assert_eq!(eclass.unwrap().to_string(), name);
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
