use std::{
    collections::{BTreeMap, HashSet},
    fs, io,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};

use super::{RepoSet, Repository};
use crate::types::FxHashSet;
use crate::{SysConf, repository::RepoName};

/// Owns a temporary directory together with a value that depends on it.
pub struct Temp<T> {
    value: T,
    _temp_dir: tempfile::TempDir,
}

impl<T> Temp<T> {
    fn new(value: T, temp_dir: tempfile::TempDir) -> Self {
        Self {
            value,
            _temp_dir: temp_dir,
        }
    }
}

impl<T> Deref for Temp<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T> DerefMut for Temp<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

enum FixtureEntry {
    File { path: PathBuf, contents: String },
    Directory { path: PathBuf, contents: String },
}

fn write_file(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

#[derive(Default)]
pub struct RepoBuilder {
    repo_name: RepoName,
    repos_conf: BTreeMap<String, String>,
    masters: Option<Vec<String>>,
    formats: Option<Vec<String>>,
    eapi: Option<String>,
    profiles: Vec<PathBuf>,
    entries: Vec<FixtureEntry>,
    categories: FxHashSet<String>,
    eclasses: Vec<String>,
    ebuilds: Vec<(String, String, String, String)>,
}

impl RepoBuilder {
    /// Creates a new [`RepositoryBuilder`] definition with the given `repo_name`.
    pub fn new(repo_name: impl Into<String>) -> Self {
        Self {
            repo_name: repo_name.into().parse().expect("invalid repository name"),
            ..Default::default()
        }
    }

    /// Sets the `masters` property in repos.conf.
    /// Use [`masters_override`](Self::masters_override) to explicitly set an empty value.
    pub fn masters(mut self, masters: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.masters = Some(masters.into_iter().map(Into::into).collect());
        self
    }

    /// Explicitly sets an empty `masters` property in repos.conf, overriding any layout.conf masters.
    pub fn masters_override(mut self) -> Self {
        self.repos_conf.insert("masters".into(), String::new());
        self
    }

    pub fn formats(mut self, formats: impl IntoIterator<Item = impl Into<String>>) -> Self {
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

    pub fn parents(
        self,
        profile: impl AsRef<Path>,
        parents: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
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

    pub fn categories(mut self, categories: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.categories = categories.into_iter().map(Into::into).collect();
        self
    }

    pub fn eclass(mut self, eclass: impl Into<String>) -> Self {
        self.eclasses.push(eclass.into());
        self
    }

    /// Adds an ebuild file with the given content to the fixture.
    pub fn ebuild(
        mut self,
        category: impl Into<String>,
        package: impl Into<String>,
        version: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        self.ebuilds.push((
            category.into(),
            package.into(),
            version.into(),
            content.into(),
        ));
        self
    }

    /// Adds a custom property to the repos.conf section for this repository.
    pub fn repos_conf_property(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.repos_conf.insert(key.into(), value.into());
        self
    }

    /// Writes the repository tree to `location`.
    pub fn write_to(self, location: impl AsRef<Path>) -> io::Result<()> {
        let location = location.as_ref();
        let metadata = location.join("metadata");
        let profiles = location.join("profiles");
        let eclasses = location.join("eclass");
        fs::create_dir_all(&metadata)?;
        fs::create_dir_all(&profiles)?;
        fs::create_dir_all(&eclasses)?;

        let mut layout = format!(
            "name = {}\nmasters = {}\n",
            self.repo_name,
            self.masters.map(|m| m.join(" ")).unwrap_or_default()
        );
        if let Some(formats) = self.formats {
            layout.push_str(&format!("profile-formats = {}\n", formats.join(" ")));
        }
        write_file(metadata.join("layout.conf"), layout)?;
        write_file(profiles.join("repo_name"), format!("{}\n", self.repo_name))?;
        if let Some(eapi) = self.eapi {
            write_file(profiles.join("eapi"), format!("{eapi}\n"))?;
        }
        write_file(
            profiles.join("profiles.desc"),
            "amd64 default/linux stable\n",
        )?;
        write_file(profiles.join("arch.list"), "amd64\n")?;
        let mut categories = self.categories.into_iter().collect::<Vec<_>>();
        categories.sort_unstable();
        write_file(profiles.join("categories"), categories.join("\n"))?;

        for profile in self.profiles {
            fs::create_dir_all(profiles.join(profile))?;
        }

        for entry in self.entries {
            match entry {
                FixtureEntry::File { path, contents } => {
                    let path = location.join(path);
                    write_file(path, contents)?;
                }
                FixtureEntry::Directory { path, contents } => {
                    let path = location.join(path);
                    write_file(path.join("entries"), contents)?;
                }
            }
        }

        for eclass in self.eclasses {
            write_file(eclasses.join(format!("{eclass}.eclass")), "")?;
        }

        for (category, package, version, content) in self.ebuilds {
            let package_dir = location.join(&category).join(&package);
            write_file(
                package_dir.join(format!("{package}-{version}.ebuild")),
                content,
            )?;
        }

        Ok(())
    }

    /// Writes and loads a repository in an owned temporary directory.
    pub fn finalize(self) -> anyhow::Result<Temp<Repository>> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let name = self.repo_name.clone();
        let location = temp_dir.path().join(name.as_str());

        self.write_to(&location)
            .with_context(|| format!("failed to write repository '{name}'"))?;
        let repository = Repository::load(&name, &location, SysConf::default().into())?;
        Ok(Temp::new(repository, temp_dir))
    }
}

/// Creates a temporary [`RepoSet`] from repository definitions.
///
/// The returned [`Temp`] keeps the repositories' temporary filesystem alive.
pub fn repo_set(repos: impl IntoIterator<Item = RepoBuilder>) -> anyhow::Result<Temp<RepoSet>> {
    let temp = tempfile::tempdir().context("failed to create temp dir")?;
    let mut names = HashSet::new();
    let mut conf = String::new();

    for repo in repos {
        let name = repo.repo_name.clone();
        if !names.insert(name.clone()) {
            bail!("tried to insert duplicate repository {name}");
        }

        let location = temp.path().join("repositories").join(name.as_str());
        conf.push_str(&format!("[{name}]\nlocation = {}\n", location.display()));
        for (key, value) in &repo.repos_conf {
            conf.push_str(&format!("{key} = {value}\n"));
        }
        conf.push('\n');

        repo.write_to(&location)
            .with_context(|| format!("failed to write repository '{name}'"))?;
    }

    let sysconf = SysConf::new(temp.path().to_path_buf());
    let repos_conf = sysconf.portage_conf().join("repos.conf");
    write_file(repos_conf, conf).context("failed to write repos.conf")?;
    let repo_set = RepoSet::new(sysconf.into())?;

    Ok(Temp::new(repo_set, temp))
}
