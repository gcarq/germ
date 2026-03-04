mod cache;
mod config;
mod desc;
pub mod eclass;
pub mod manager;
mod sync;

use crate::deps::Atom;
use crate::eapi::Eapi;
use crate::ebuild::Ebuild;
use crate::linefile::LineBasedFile;
use crate::makenv::MakeEnv;
use crate::package::Package;
use crate::package::version::PackageVersion;
use crate::regex::PKG_VER_REV;
use crate::repository::cache::MetadataCache;
use crate::repository::config::RepositoryConfig;
use crate::repository::desc::ProfileDescription;
use crate::repository::eclass::Eclasses;
use crate::repository::sync::{SyncHandler, build_sync_handler};
use crate::utils;
use crate::utils::FileFromPath;
use anyhow::{Context, Result, anyhow};
use log::{debug, info, trace, warn};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::LazyLock;
use std::{fmt, fs, io};

/// Regex to validate and parse `package`, `version`, `suffixes` and the `revision`
/// from an ebuild name.
static EBUILD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^{PKG_VER_REV}.ebuild$")).unwrap());

/// Represents a package repository with its location, name, eapi version, categories, packages,
/// and other metadata. The repository will be synced using a [`SyncHandler`].
#[derive(Default)]
pub struct Repository {
    pub location: PathBuf,
    pub name: String,
    masters: Vec<String>,
    pub eapi: Eapi,
    categories: Vec<String>,
    /// Maps packages to their ebuild file paths.
    packages: HashMap<Package, PathBuf>,
    pub package_mask: LineBasedFile,
    pub package_unmask: LineBasedFile,
    pub eclasses: Eclasses,
    pub arch_list: LineBasedFile,
    pub profiles_desc: Vec<ProfileDescription>,

    metadata_cache: Option<MetadataCache>,
    sync_handler: Option<Box<dyn SyncHandler>>,
}

impl Repository {
    /// Builds a new [`Repository`] with the given `location` and INI `properties` from repos.conf.
    ///
    /// Packages must be collected separately by calling `collect_packages` since they require
    /// parsing the whole repository and can be expensive to build.
    /// This allows deferring package collection until it's actually needed.
    pub fn new(config: &RepositoryConfig) -> Result<Self> {
        let location = config.location.canonicalize()?;

        let eapi = Self::read_eapi(&location)?;
        let profiles = location.join("profiles");
        let repository = Self {
            packages: HashMap::new(),
            categories: Vec::new(),
            package_mask: LineBasedFile::from_path(
                &profiles.join("package.mask"),
                eapi.supports_profile_file_dirs(),
                true,
            )?,
            package_unmask: LineBasedFile::from_path(
                &profiles.join("package.unmask"),
                eapi.supports_profile_file_dirs(),
                true,
            )?,
            eclasses: Eclasses::default(),
            arch_list: LineBasedFile::from_path(&profiles.join("arch.list"), false, true)?,
            profiles_desc: LineBasedFile::from_path(&profiles.join("profiles.desc"), false, true)?
                .into_iter()
                .map(|line| ProfileDescription::from_line(&line))
                .collect::<Result<_>>()?,
            masters: config.masters.clone(),
            name: config.name.clone(),
            location,
            sync_handler: build_sync_handler(&config.raw_properties)?,
            eapi,
            metadata_cache: None,
        };
        Ok(repository)
    }

    /// Populates all categories and packages. Categories are inherited from the given `masters`.
    pub fn populate(&mut self, masters: &[&Repository]) -> Result<()> {
        self.categories = self
            .collect_categories(masters)
            .with_context(|| "unable to collect categories")?;
        self.packages = self
            .collect_packages()
            .with_context(|| "unable to collect packages")?;
        self.eclasses = self
            .collect_eclasses(masters)
            .with_context(|| "unable to collect eclasses")?;
        Ok(())
    }

    /// Generates metadata cache for all packages and serializes it to disk.
    /// If `force` is true, metadata will be generated regardless of cache state.
    pub fn generate_metadata(&mut self, make_env: &MakeEnv, force: bool) -> Result<()> {
        info!("Generating metadata cache for {self} ...");
        self.load_cache()?;
        // Safe to unwrap since we loaded the cache right before this
        let cache = self.metadata_cache.as_ref().unwrap();

        let metadata = self
            .packages
            .iter()
            .filter(|(_, path)| force || !cache.is_cached(path))
            .map(|(pkg, path)| {
                let ebuild = Ebuild::new(path, pkg, self)
                    .with_context(|| anyhow!("failed to construct ebuild for {pkg}"))?;
                let metadata = ebuild
                    .generate_metadata(make_env)
                    .with_context(|| anyhow!("unable to generate metadata for {ebuild}"))?;
                Ok((ebuild.path.to_path_buf(), metadata))
            })
            .collect::<Result<HashMap<_, _>>>()?;

        // Safe to unwrap since we loaded the cache in this function
        let cache_mut = self.metadata_cache.as_mut().unwrap();
        cache_mut.extend(metadata);

        self.write_cache()
            .with_context(|| anyhow!("unable to write cache for {self} to disk"))?;

        info!("Updated metadata cache for {self}");
        Ok(())
    }

    /// Returns an `Iterator` over all packages in the repository.
    /// TODO: Order the returned packages by version
    pub fn packages(&self) -> impl Iterator<Item = &Package> {
        self.packages.keys()
    }

    /// Returns all packages in the repository that match the given `atom`.
    /// TODO: Order the returned packages by version
    /// TODO: Consider returning an iterator
    pub fn find_packages(&self, atom: &Atom) -> Vec<&Package> {
        self.packages().filter(|pkg| atom.matches(pkg)).collect()
    }

    /// Checks if the profile with the relative `profile_path` is valid for the given `arch`.
    /// The repository location prefix must be stripped from the passed `profile_path` string
    /// e.g.: default/linux/23.0
    pub fn is_known_profile(&self, arch: &str, profile_path: &str) -> bool {
        self.profiles_desc
            .iter()
            .any(|desc| desc.keyword == arch && desc.profile_path == profile_path)
    }

    /// Synchronizes the repository using its [`SyncHandler`].
    pub fn sync(&self) -> Result<()> {
        if let Some(sync_handler) = &self.sync_handler {
            info!("Syncing repository '{}'", self.name);
            sync_handler.sync()?;
        }
        Ok(())
    }

    /// Collects and returns all categories from the repo `location`.
    /// Categories from the given `masters` are inherited and added to the collected categories.
    fn collect_categories(&self, masters: &[&Repository]) -> Result<Vec<String>> {
        let mut categories = masters
            .iter()
            .flat_map(|repo| &repo.categories)
            .cloned()
            .collect::<Vec<String>>();

        let path = self.location.join("profiles").join("categories");
        if path.exists() {
            categories.extend(
                fs::read_to_string(&path)
                    .with_context(|| anyhow!("unable to read '{}'", path.display()))?
                    .lines()
                    .map(ToOwned::to_owned),
            );
        }
        Ok(categories)
    }

    /// Collects all packages from the repository.
    /// [`Self::collect_categories`] must be called before calling this method since only
    /// known categories are considered when collecting packages.
    fn collect_packages(&self) -> Result<HashMap<Package, PathBuf>> {
        let mut packages = HashMap::new();
        for category in &self.categories {
            let cat_path = self.location.join(category);

            let Ok(pkg_paths) = utils::list_dirs(&cat_path) else {
                continue;
            };

            for pkg_path in pkg_paths {
                let pkg_path = pkg_path?;
                let pkg_name = utils::path_to_filename(&pkg_path)?;

                for ebuild_path in utils::list_files(&pkg_path)? {
                    let ebuild_path = ebuild_path?;
                    let caps = match EBUILD_RE.captures(utils::path_to_filename(&ebuild_path)?) {
                        Some(caps) if caps["package"].starts_with(pkg_name) => caps,
                        _ => continue,
                    };
                    let version = PackageVersion::new(
                        &caps["version"],
                        Some(&caps["suffixes"]),
                        caps.name("revision").map(|m| m.as_str()),
                    )?;
                    let pkg = Package::new(
                        utils::path_to_filename(&cat_path)?,
                        pkg_name,
                        version,
                        &self.name,
                    )?;
                    packages.insert(pkg, ebuild_path);
                }
            }
        }
        packages.shrink_to_fit();
        Ok(packages)
    }

    /// Collects all eclasses from the repo `location` and its `masters`.
    fn collect_eclasses(&self, masters: &[&Repository]) -> Result<Eclasses> {
        let mut eclasses = Eclasses::from_path(&self.location.join("eclass"))?;

        for master in masters {
            trace!("Extending eclasses for '{self}' from repository '{master}'");
            eclasses.extend(&master.eclasses);
        }
        Ok(eclasses)
    }

    /// Loads the metadata cache from disk and assigns it to `metadata_cache`.
    fn load_cache(&mut self) -> Result<()> {
        let path = PathBuf::from(format!("/tmp/package-manager/metadata/{self}"));
        debug!(
            "Loading metadata cache for {self} from {} ...",
            path.display()
        );
        let reader = match File::open(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                warn!("missing metadata cache for {self}");
                self.metadata_cache = Some(MetadataCache::default());
                return Ok(());
            }
            Err(err) => Err(anyhow!("unable to open metadata cache from disk: {err}"))?,
        };
        let mut cache = MetadataCache::deserialize(reader)?;
        cache.retain(&self.packages.values().collect::<HashSet<_>>());
        self.metadata_cache = Some(cache);
        Ok(())
    }

    /// Writes the metadata cache to disk.
    /// TODO: replace hardcoded testing path
    fn write_cache(&self) -> Result<()> {
        let cache = self
            .metadata_cache
            .as_ref()
            .ok_or_else(|| anyhow!("metadata cache is not loaded"))?;

        let path = PathBuf::from(format!("/tmp/package-manager/metadata/{self}"));
        debug!(
            "Writing metadata cache for {self} to {} ...",
            path.display()
        );
        let writer = File::create(&path).with_context(|| {
            anyhow!(
                "unable to create metadata cache file at '{}'",
                path.display()
            )
        })?;
        cache.serialize(writer)
    }

    /// Reads the repository eapi version from the given repository `path`.
    /// Returns `Eapi::default()` if no eapi file exists.
    fn read_eapi(path: &Path) -> Result<Eapi> {
        let eapi_file = path.join("profiles").join("eapi");
        if !fs::exists(&eapi_file)? {
            return Ok(Eapi::default());
        }
        Eapi::from_str(
            fs::read_to_string(&eapi_file)?
                .lines()
                .next()
                .ok_or_else(|| anyhow!("Empty eapi file"))?,
        )
    }
}

impl Eq for Repository {}

impl PartialEq for Repository {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Hash for Repository {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl fmt::Display for Repository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ebuild_regex_match() {
        let valid_ebuilds = [
            "vim-8.2.3456.ebuild",
            "vim-8.2.3456-r1.ebuild",
            "rust-1.65.0_alpha1-r2.ebuild",
            "curl-7.79.1_beta2_p20220101.ebuild",
        ];
        for ebuild in valid_ebuilds {
            assert!(
                EBUILD_RE.is_match(ebuild),
                "ebuild name '{ebuild}' should be valid",
            );
        }
    }

    #[test]
    fn test_ebuild_regex_no_match() {
        let invalid_ebuilds = [
            "",
            "vim8.2.3456.ebuild",
            "app-editors/vim-.ebuild",
            "dev-lang/rust-1.65.0_alphaX-r2.ebuild",
            "net-misc/curl-7.79.1--r1.ebuild",
            "net-misc/curl-7.79.1_beta2_p20220101-rX.ebuild",
        ];
        for ebuild in invalid_ebuilds {
            assert!(
                !EBUILD_RE.is_match(ebuild),
                "ebuild name '{ebuild}' should be invalid",
            );
        }
    }
}
