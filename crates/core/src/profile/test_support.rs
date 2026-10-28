use std::path::{Path, PathBuf};
use std::{fs, io};

use anyhow::Context;

use super::Profile;
use crate::files::entry::{Entry, EntryValue, LineOrigin, Precedence};
use crate::files::pkgfile::KeywordRule;
use crate::repository::RepoSet;
use crate::test_support::{FixtureFile, write_file};

/// Builds a profile directory in a repository fixture.
pub struct ProfileBuilder {
    name: PathBuf,
    parents: Vec<String>,
    files: Vec<FixtureFile>,
}

impl ProfileBuilder {
    /// Creates a profile named `name` below the repository's `profiles` directory.
    pub fn new(name: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            parents: Vec::new(),
            files: Vec::new(),
        }
    }

    /// Adds `parent` to the profile's `parent` file.
    pub fn parent(mut self, parent: impl Into<String>) -> Self {
        self.parents.push(parent.into());
        self
    }

    /// Writes the profile's `eapi` file.
    pub fn eapi(self, eapi: impl Into<String>) -> Self {
        self.file("eapi", format!("{}\n", eapi.into()))
    }

    /// Adds a file relative to the profile directory.
    pub fn file(mut self, path: impl AsRef<Path>, contents: impl Into<String>) -> Self {
        self.files.push(FixtureFile::file(path.as_ref(), contents));
        self
    }

    /// Adds a file-directory entry relative to the profile directory.
    pub fn entries_dir(mut self, path: impl AsRef<Path>, contents: impl Into<String>) -> Self {
        self.files
            .push(FixtureFile::entries_dir(path.as_ref(), contents));
        self
    }

    /// Writes the profile directory below `profiles_root`.
    pub fn write_to(&self, profiles_root: &Path) -> io::Result<()> {
        let root = profiles_root.join(&self.name);
        fs::create_dir_all(&root)?;
        if !self.parents.is_empty() {
            write_file(root.join("parent"), self.parents.join("\n"))?;
        }
        for file in &self.files {
            file.write_to(&root)?;
        }
        Ok(())
    }
}

impl From<&str> for ProfileBuilder {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

/// Builds the path to `profile` in the repository at `location`.
pub fn profile_path(location: &Path, profile: &str) -> PathBuf {
    location.join("profiles").join(profile)
}

/// The expected value, precedence and line number of an entry in a profile file.
#[derive(Clone, Copy)]
pub struct ProfileEntry<'a> {
    profile: &'a str,
    value: &'a str,
    prec: Precedence,
    lineno: u32,
}

impl<'a> ProfileEntry<'a> {
    /// Creates an expected entry for `value` read at `lineno` in `profile`.
    pub const fn new(profile: &'a str, value: &'a str, prec: Precedence, lineno: u32) -> Self {
        Self {
            profile,
            value,
            prec,
            lineno,
        }
    }
}

/// Builds the [`Entry`] expected at `file` in `profile` under `location`.
pub fn profile_entry<T: EntryValue>(
    location: &Path,
    file: &str,
    entry: ProfileEntry<'_>,
) -> Entry<T> {
    let path = profile_path(location, entry.profile).join(file);
    Entry::new(
        entry.value,
        entry.prec,
        LineOrigin::new(path.into(), entry.lineno),
    )
    .unwrap()
}

/// Builds the [`Entry`] list expected at `file` in the profile chain under `location`.
pub fn profile_entries<T: EntryValue>(
    location: &Path,
    file: &str,
    entries: &[ProfileEntry<'_>],
) -> Vec<Entry<T>> {
    entries
        .iter()
        .map(|&entry| profile_entry(location, file, entry))
        .collect()
}

/// Builds the [`KeywordRule`] list expected at `file` in the profile chain under `location`.
pub fn keyword_selectors(
    location: &Path,
    file: &str,
    entries: &[ProfileEntry<'_>],
) -> Vec<KeywordRule> {
    profile_entries(location, file, entries)
        .into_iter()
        .map(KeywordRule::Selector)
        .collect()
}

/// Resolves the profile at `profile` in the repository named `repo`.
pub fn resolve_profile(reposet: &RepoSet, repo: &str, profile: &str) -> anyhow::Result<Profile> {
    let location = reposet
        .get(repo)
        .with_context(|| format!("repository '{repo}' not found"))?
        .location();
    reposet.resolve_profile(&profile_path(location, profile))
}
