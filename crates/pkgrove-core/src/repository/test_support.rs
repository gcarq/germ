use std::fs;
use std::io;
use std::path::PathBuf;

pub(crate) struct RepositoryFixture {
    location: PathBuf,
    repo_name: String,
    layout_name: Option<String>,
    masters: Vec<String>,
    categories: Vec<String>,
    eclasses: Vec<String>,
}

impl RepositoryFixture {
    pub(crate) fn new(location: impl Into<PathBuf>, repo_name: impl Into<String>) -> Self {
        let repo_name = repo_name.into();
        Self {
            location: location.into(),
            layout_name: Some(repo_name.clone()),
            repo_name,
            masters: Vec::new(),
            categories: Vec::new(),
            eclasses: Vec::new(),
        }
    }

    pub(crate) fn layout_name(mut self, name: impl Into<String>) -> Self {
        self.layout_name = Some(name.into());
        self
    }

    pub(crate) fn without_layout_name(mut self) -> Self {
        self.layout_name = None;
        self
    }

    pub(crate) fn masters<I, S>(mut self, masters: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.masters = masters.into_iter().map(Into::into).collect();
        self
    }

    pub(crate) fn categories<I, S>(mut self, categories: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.categories = categories.into_iter().map(Into::into).collect();
        self
    }

    pub(crate) fn eclass(mut self, eclass: impl Into<String>) -> Self {
        self.eclasses.push(eclass.into());
        self
    }

    pub(crate) fn write(self) -> io::Result<PathBuf> {
        let metadata = self.location.join("metadata");
        let profiles = self.location.join("profiles");
        let eclasses = self.location.join("eclass");
        fs::create_dir_all(&metadata)?;
        fs::create_dir_all(&profiles)?;
        fs::create_dir_all(&eclasses)?;

        let layout_name = self
            .layout_name
            .map(|name| format!("name = {name}\n"))
            .unwrap_or_default();
        fs::write(
            metadata.join("layout.conf"),
            format!("{layout_name}masters = {}\n", self.masters.join(" ")),
        )?;
        fs::write(profiles.join("repo_name"), format!("{}\n", self.repo_name))?;
        fs::write(
            profiles.join("profiles.desc"),
            "amd64 default/linux stable\n",
        )?;
        fs::write(profiles.join("arch.list"), "amd64\n")?;
        fs::write(profiles.join("categories"), self.categories.join("\n"))?;

        for eclass in self.eclasses {
            fs::write(eclasses.join(format!("{eclass}.eclass")), "")?;
        }

        Ok(self.location)
    }
}
