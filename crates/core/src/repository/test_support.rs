use std::{
    collections::HashMap,
    fs, io,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail};

use super::RepoSet;
use crate::types::FxHashSet;

enum FixtureEntry {
    File { path: PathBuf, contents: String },
    Directory { path: PathBuf, contents: String },
}

#[derive(Default)]
pub struct RepositoryFixture {
    pub(crate) location: PathBuf,
    pub(crate) repo_name: String,
    masters: Option<Vec<String>>,
    formats: Option<Vec<String>>,
    eapi: Option<String>,
    profiles: Vec<PathBuf>,
    entries: Vec<FixtureEntry>,
    categories: FxHashSet<String>,
    eclasses: Vec<String>,
    ebuilds: Vec<(String, String, String)>,
    pub(crate) repos_conf_properties: HashMap<String, String>,
}

impl RepositoryFixture {
    /// Creates a new [`RepositoryFixture`] with a temporary location and the given `repo_name`.
    ///
    /// # Panics
    ///
    /// Will panic if the temporary directory cannot be created.
    pub fn new(repo_name: &str) -> Self {
        let temp_dir = tempfile::Builder::new()
            .tempdir()
            .expect("failed to create temp dir");
        Self {
            location: temp_dir.path().join("repo").join(repo_name),
            repo_name: repo_name.into(),
            ..Default::default()
        }
    }

    /// Sets the location for standalone use (outside of [`RepoSetFixture`]).
    pub fn with_location(location: impl Into<PathBuf>, repo_name: &str) -> Self {
        Self {
            location: location.into(),
            repo_name: repo_name.into(),
            ..Default::default()
        }
    }

    /// Sets the `masters` property in repos.conf.
    /// Use [`masters_override`](Self::masters_override) to explicitly set an empty value.
    pub fn masters<I, S>(mut self, masters: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.masters = Some(masters.into_iter().map(Into::into).collect());
        self
    }

    /// Explicitly sets an empty `masters` property in repos.conf, overriding any layout.conf masters.
    pub fn masters_override(mut self) -> Self {
        self.repos_conf_properties
            .insert("masters".into(), String::new());
        self
    }

    pub fn formats<I, S>(mut self, formats: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.formats = Some(formats.into_iter().map(Into::into).collect());
        self
    }

    /// Sets the repository default EAPI (written to `profiles/eapi`).
    pub fn eapi(mut self, eapi: impl Into<String>) -> Self {
        self.eapi = Some(eapi.into());
        self
    }

    pub fn profile(mut self, profile: impl Into<PathBuf>) -> Self {
        self.profiles.push(profile.into());
        self
    }

    pub fn profile_eapi(self, profile: impl AsRef<Path>, eapi: impl Into<String>) -> Self {
        self.profile_file(profile.as_ref().join("eapi"), format!("{}\n", eapi.into()))
    }

    pub fn parents<I, S>(self, profile: impl AsRef<Path>, parents: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let parents = parents.into_iter().map(Into::into).collect::<Vec<_>>();
        self.profile_file(profile.as_ref().join("parent"), parents.join("\n"))
    }

    /// Adds a profile file by creating `profiles/<path>`.
    pub fn profile_file(mut self, path: impl AsRef<Path>, contents: impl Into<String>) -> Self {
        self.entries.push(FixtureEntry::File {
            path: Path::new("profiles").join(path.as_ref()),
            contents: contents.into(),
        });
        self
    }

    /// Adds a profile file-directory entry by creating `profiles/<path>/entries`.
    pub fn profile_entries_dir(
        mut self,
        path: impl AsRef<Path>,
        contents: impl Into<String>,
    ) -> Self {
        self.entries.push(FixtureEntry::Directory {
            path: Path::new("profiles").join(path.as_ref()),
            contents: contents.into(),
        });
        self
    }

    pub fn categories<I, S>(mut self, categories: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.categories = categories.into_iter().map(Into::into).collect();
        self
    }

    pub fn eclass(mut self, eclass: impl Into<String>) -> Self {
        self.eclasses.push(eclass.into());
        self
    }

    /// Adds a minimal ebuild file to the fixture.
    pub fn ebuild(
        mut self,
        category: impl Into<String>,
        package: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        self.ebuilds
            .push((category.into(), package.into(), version.into()));
        self
    }

    /// Adds a custom property to the repos.conf section for this repository.
    pub fn repos_conf_property(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.repos_conf_properties.insert(key.into(), value.into());
        self
    }

    pub fn write(self) -> io::Result<PathBuf> {
        let metadata = self.location.join("metadata");
        let profiles = self.location.join("profiles");
        let eclasses = self.location.join("eclass");
        fs::create_dir_all(&metadata)?;
        fs::create_dir_all(&profiles)?;
        fs::create_dir_all(&eclasses)?;

        let masters = self.masters.map(|m| m.join(" ")).unwrap_or_default();
        let mut layout = format!("name = {}\nmasters = {}\n", self.repo_name, masters);
        if let Some(formats) = self.formats {
            layout.push_str(&format!("profile-formats = {}\n", formats.join(" ")));
        }
        fs::write(metadata.join("layout.conf"), layout)?;
        fs::write(profiles.join("repo_name"), format!("{}\n", self.repo_name))?;
        if let Some(eapi) = self.eapi {
            fs::write(profiles.join("eapi"), format!("{eapi}\n"))?;
        }
        fs::write(
            profiles.join("profiles.desc"),
            "amd64 default/linux stable\n",
        )?;
        fs::write(profiles.join("arch.list"), "amd64\n")?;
        let mut categories = self.categories.into_iter().collect::<Vec<_>>();
        categories.sort_unstable();
        fs::write(profiles.join("categories"), categories.join("\n"))?;

        for profile in self.profiles {
            fs::create_dir_all(profiles.join(profile))?;
        }

        for entry in self.entries {
            match entry {
                FixtureEntry::File { path, contents } => {
                    let path = self.location.join(path);
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(path, contents)?;
                }
                FixtureEntry::Directory { path, contents } => {
                    let path = self.location.join(path);
                    fs::create_dir_all(&path)?;
                    fs::write(path.join("entries"), contents)?;
                }
            }
        }

        for eclass in self.eclasses {
            fs::write(eclasses.join(format!("{eclass}.eclass")), "")?;
        }

        for (category, package, version) in self.ebuilds {
            let package_dir = self.location.join(&category).join(&package);
            fs::create_dir_all(&package_dir)?;
            fs::write(package_dir.join(format!("{package}-{version}.ebuild")), "")?;
        }

        Ok(self.location)
    }
}

/// A test fixture that creates a temporary directory, writes multiple repositories,
/// generates a `repos.conf`, and loads a [`RepoSet`].
///
/// The returned struct implements [`Deref`] to [`RepoSet`] and keeps the temporary
/// directory alive.
///
/// # Example
///
/// ```
/// let fixture = RepoSetFixture::new(vec![
///     RepositoryFixture::new("master")
///         .categories(["app-misc"])
///         .eclass("my_eclass"),
///     RepositoryFixture::new("overlay")
///         .masters(["master"]),
/// ]).unwrap();
///
/// // Use as RepoSet
/// let overlay = fixture.get("overlay").unwrap();
///
/// // Access individual repo paths
/// let master_path = fixture.get_repo_path("master").unwrap();
/// ```
pub struct RepoSetFixture {
    repo_set: RepoSet,
    _temp: tempfile::TempDir,
    paths: HashMap<String, PathBuf>,
}

impl RepoSetFixture {
    /// Creates a new [`RepoSetFixture`] from the given [`RepositoryFixture`] instances.
    ///
    /// Each repository will be written to a subdirectory named after its repo name
    /// inside a temporary directory. A `repos.conf` is generated and the [`RepoSet`]
    /// is loaded.
    pub fn new(repositories: Vec<RepositoryFixture>) -> anyhow::Result<Self> {
        let temp = tempfile::Builder::new()
            .tempdir()
            .map_err(|e| anyhow!("failed to create temp dir: {e}"))?;

        // Collect paths and repos.conf properties before consuming the fixtures
        let mut paths = HashMap::new();
        let mut conf_props: HashMap<String, HashMap<String, String>> = HashMap::new();
        for repo in &repositories {
            let location = temp.path().join(&repo.repo_name);
            paths.insert(repo.repo_name.clone(), location);
            if !repo.repos_conf_properties.is_empty()
                && conf_props
                    .insert(repo.repo_name.clone(), repo.repos_conf_properties.clone())
                    .is_some()
            {
                bail!("tried to insert duplicate repository {}", repo.repo_name);
            }
        }

        // Assign locations and write each repository
        for mut repo in repositories {
            let repo_name = repo.repo_name.clone();
            repo.location = paths[&repo_name].clone();
            repo.write()
                .map_err(|e| anyhow!("failed to write repository '{repo_name}': {e}"))?;
        }

        // Build and write repos.conf
        let mut conf = String::new();
        for (name, location) in &paths {
            conf.push_str(&format!("[{name}]\nlocation = {}\n", location.display()));
            if let Some(props) = conf_props.get(name) {
                for (key, value) in props {
                    conf.push_str(&format!("{key} = {value}\n"));
                }
            }
            conf.push('\n');
        }

        let repos_conf = temp.path().join("repos.conf");
        fs::write(&repos_conf, conf).map_err(|e| anyhow!("failed to write repos.conf: {e}"))?;

        let repo_set = RepoSet::new(&repos_conf)?;

        Ok(Self {
            repo_set,
            _temp: temp,
            paths,
        })
    }

    /// Returns the `Path` for the given `repo_name`
    pub fn get_repo_path(&self, repo_name: &str) -> Option<&Path> {
        self.paths.get(repo_name).map(PathBuf::as_path)
    }
}

impl Deref for RepoSetFixture {
    type Target = RepoSet;

    fn deref(&self) -> &Self::Target {
        &self.repo_set
    }
}

impl DerefMut for RepoSetFixture {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.repo_set
    }
}
