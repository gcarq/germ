use std::fs;
use std::io;
use std::path::{Path, PathBuf};

enum FixtureEntry {
    File { path: PathBuf, contents: String },
    Directory { path: PathBuf, contents: String },
}

pub struct RepositoryFixture {
    location: PathBuf,
    repo_name: String,
    masters: Vec<String>,
    profile_formats: Option<Vec<String>>,
    profiles_eapi: Option<String>,
    profiles: Vec<PathBuf>,
    entries: Vec<FixtureEntry>,
    categories: Vec<String>,
    eclasses: Vec<String>,
}

impl RepositoryFixture {
    pub fn new(location: impl Into<PathBuf>, repo_name: impl Into<String>) -> Self {
        let repo_name = repo_name.into();
        Self {
            location: location.into(),
            repo_name,
            masters: Vec::new(),
            profile_formats: None,
            profiles_eapi: None,
            profiles: Vec::new(),
            entries: Vec::new(),
            categories: Vec::new(),
            eclasses: Vec::new(),
        }
    }

    pub fn masters<I, S>(mut self, masters: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.masters = masters.into_iter().map(Into::into).collect();
        self
    }

    pub fn profile_formats<I, S>(mut self, formats: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.profile_formats = Some(formats.into_iter().map(Into::into).collect());
        self
    }

    pub fn profiles_eapi(mut self, eapi: impl Into<String>) -> Self {
        self.profiles_eapi = Some(eapi.into());
        self
    }

    pub fn profile(mut self, profile: impl Into<PathBuf>) -> Self {
        self.profiles.push(profile.into());
        self
    }

    pub fn profile_eapi(self, profile: impl AsRef<Path>, eapi: impl Into<String>) -> Self {
        self.profile_file(profile, "eapi", format!("{}\n", eapi.into()))
    }

    pub fn profile_parents<I, S>(self, profile: impl AsRef<Path>, parents: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let parents = parents.into_iter().map(Into::into).collect::<Vec<_>>();
        self.profile_file(profile, "parent", parents.join("\n"))
    }

    fn repository_file(mut self, name: impl Into<PathBuf>, contents: impl Into<String>) -> Self {
        self.entries.push(FixtureEntry::File {
            path: name.into(),
            contents: contents.into(),
        });
        self
    }

    pub fn repository_directory(
        mut self,
        name: impl Into<PathBuf>,
        contents: impl Into<String>,
    ) -> Self {
        self.entries.push(FixtureEntry::Directory {
            path: name.into(),
            contents: contents.into(),
        });
        self
    }

    fn profile_file(
        self,
        profile: impl AsRef<Path>,
        name: impl AsRef<Path>,
        contents: impl Into<String>,
    ) -> Self {
        self.repository_file(profile.as_ref().join(name), contents)
    }

    pub fn profile_directory(
        self,
        profile: impl AsRef<Path>,
        name: impl AsRef<Path>,
        contents: impl Into<String>,
    ) -> Self {
        self.repository_directory(profile.as_ref().join(name), contents)
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

    pub fn write(self) -> io::Result<PathBuf> {
        let metadata = self.location.join("metadata");
        let profiles = self.location.join("profiles");
        let eclasses = self.location.join("eclass");
        fs::create_dir_all(&metadata)?;
        fs::create_dir_all(&profiles)?;
        fs::create_dir_all(&eclasses)?;

        let mut layout = format!(
            "name = {}\nmasters = {}\n",
            self.repo_name,
            self.masters.join(" ")
        );
        if let Some(formats) = self.profile_formats {
            layout.push_str(&format!("profile-formats = {}\n", formats.join(" ")));
        }
        fs::write(metadata.join("layout.conf"), layout)?;
        fs::write(profiles.join("repo_name"), format!("{}\n", self.repo_name))?;
        if let Some(eapi) = self.profiles_eapi {
            fs::write(profiles.join("eapi"), format!("{eapi}\n"))?;
        }
        fs::write(
            profiles.join("profiles.desc"),
            "amd64 default/linux stable\n",
        )?;
        fs::write(profiles.join("arch.list"), "amd64\n")?;
        fs::write(profiles.join("categories"), self.categories.join("\n"))?;

        for profile in self.profiles {
            fs::create_dir_all(profiles.join(profile))?;
        }

        for entry in self.entries {
            match entry {
                FixtureEntry::File { path, contents } => {
                    let path = profiles.join(path);
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(path, contents)?;
                }
                FixtureEntry::Directory { path, contents } => {
                    let path = profiles.join(path);
                    fs::create_dir_all(&path)?;
                    fs::write(path.join("entries"), contents)?;
                }
            }
        }

        for eclass in self.eclasses {
            fs::write(eclasses.join(format!("{eclass}.eclass")), "")?;
        }

        Ok(self.location)
    }
}
