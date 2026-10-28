use std::collections::{BTreeMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::{fs, io};

use anyhow::{Context, bail};
use germ_pms::test_support::cpv;
use germ_pms::{Cpv, RepoName};

use super::{RepoSet, Repository};
use crate::profile::test_support::ProfileBuilder;
use crate::test_support::{FixtureFile, build_sysconf, write_file};
use crate::types::FxHashSet;

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

pub struct RepoBuilder {
    repo_name: RepoName,
    repos_conf: BTreeMap<String, String>,
    masters: Option<Vec<String>>,
    formats: Option<Vec<String>>,
    eapi: Option<String>,
    profiles: Vec<ProfileBuilder>,
    files: Vec<FixtureFile>,
    categories: FxHashSet<String>,
    eclasses: Vec<String>,
    licenses: Vec<String>,
    ebuilds: Vec<(Cpv, String)>,
}

impl RepoBuilder {
    /// Creates a new [`RepositoryBuilder`] definition with the given `repo_name`.
    ///
    /// # Panics
    ///
    /// Panics if the passed `repo_name` is invalid.
    pub fn new(repo_name: impl Into<String>) -> Self {
        Self {
            repo_name: repo_name.into().parse().expect("invalid repository name"),
            repos_conf: BTreeMap::new(),
            masters: None,
            formats: None,
            eapi: None,
            profiles: Vec::new(),
            files: Vec::new(),
            categories: FxHashSet::default(),
            eclasses: Vec::new(),
            licenses: Vec::new(),
            ebuilds: Vec::new(),
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

    /// Adds a profile directory to the fixture.
    pub fn profile(mut self, profile: impl Into<ProfileBuilder>) -> Self {
        self.profiles.push(profile.into());
        self
    }

    /// Adds multiple profile directories to the fixture.
    pub fn profiles(
        mut self,
        profiles: impl IntoIterator<Item = impl Into<ProfileBuilder>>,
    ) -> Self {
        self.profiles.extend(profiles.into_iter().map(Into::into));
        self
    }

    /// Adds a file relative to the repository root.
    pub fn file(mut self, path: impl AsRef<Path>, contents: impl Into<String>) -> Self {
        self.files.push(FixtureFile::file(path.as_ref(), contents));
        self
    }

    /// Adds a file-directory entry relative to the repository root.
    pub fn entries_dir(mut self, path: impl AsRef<Path>, contents: impl Into<String>) -> Self {
        self.files
            .push(FixtureFile::entries_dir(path.as_ref(), contents));
        self
    }

    pub fn categories(mut self, categories: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.categories = categories.into_iter().map(Into::into).collect();
        self
    }

    /// Adds an eclass with the given `name` to the fixture.
    pub fn eclass(mut self, name: impl Into<String>) -> Self {
        self.eclasses.push(name.into());
        self
    }

    /// Adds a license file with the given `name` to the fixture.
    pub fn license(mut self, name: impl Into<String>) -> Self {
        self.licenses.push(name.into());
        self
    }

    /// Adds an ebuild file to the fixture.
    ///
    /// # Panics
    ///
    /// Panics if `fqn` is not a valid fully qualified package name.
    pub fn ebuild(mut self, fqn: impl AsRef<str>, content: impl Into<String>) -> Self {
        self.ebuilds.push((cpv(fqn.as_ref()), content.into()));
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
            profile.write_to(&profiles)?;
        }

        for file in &self.files {
            file.write_to(location)?;
        }

        for eclass in self.eclasses {
            write_file(eclasses.join(format!("{eclass}.eclass")), "")?;
        }

        for license in self.licenses {
            write_file(location.join("licenses").join(license), "")?;
        }

        for (cpv, content) in self.ebuilds {
            let package_dir = location
                .join(cpv.category().as_str())
                .join(cpv.package().as_str());
            write_file(package_dir.join(format!("{}.ebuild", cpv.pf())), content)?;
        }

        Ok(())
    }

    /// Writes and loads a repository in an owned temporary directory.
    pub fn finalize(self) -> anyhow::Result<Temp<Repository>> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let sysconf = build_sysconf(&temp_dir);
        let name = self.repo_name.clone();
        let location = temp_dir.path().join(name.as_str());

        self.write_to(&location)
            .with_context(|| format!("failed to write repository '{name}'"))?;
        let repository = Repository::load(&name, &location, sysconf.into())?;
        Ok(Temp::new(repository, temp_dir))
    }
}

/// Creates a temporary [`RepoSet`] from repository definitions.
///
/// The returned [`Temp`] keeps the repositories' temporary filesystem alive.
pub fn repo_set(repos: impl IntoIterator<Item = RepoBuilder>) -> anyhow::Result<Temp<RepoSet>> {
    let temp = tempfile::tempdir().context("failed to create temp dir")?;
    let reposet = repo_set_in(&temp, repos)?;
    Ok(Temp::new(reposet, temp))
}

/// Creates a [`RepoSet`] from repository definitions rooted in `temp_dir`.
pub fn repo_set_in(
    temp_dir: &tempfile::TempDir,
    repos: impl IntoIterator<Item = RepoBuilder>,
) -> anyhow::Result<RepoSet> {
    let sysconf = build_sysconf(temp_dir);
    let mut names = HashSet::new();
    let mut conf = String::new();

    for repo in repos {
        let name = repo.repo_name.clone();
        if !names.insert(name.clone()) {
            bail!("tried to insert duplicate repository {name}");
        }

        let location = temp_dir.path().join("repositories").join(name.as_str());
        conf.push_str(&format!("[{name}]\nlocation = {}\n", location.display()));
        for (key, value) in &repo.repos_conf {
            conf.push_str(&format!("{key} = {value}\n"));
        }
        conf.push('\n');

        repo.write_to(&location)
            .with_context(|| format!("failed to write repository '{name}'"))?;
    }

    let repos_conf = sysconf.portage_conf().join("repos.conf");
    write_file(repos_conf, conf).context("failed to write repos.conf")?;
    Ok(RepoSet::new(sysconf.into())?)
}
