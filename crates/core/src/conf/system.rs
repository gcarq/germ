use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::thread;

const PORTAGE_CONF_PATH: &str = "etc/portage";
const DEFAULT_PORTAGE_CONF_PATH: &str = "usr/share/portage/config";
const VDB_PATH: &str = "var/db/pkg";

/// Runtime configuration shared by repository operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SysConf {
    /// Root path to configuration files, usually `/`.
    config_root: PathBuf,
    /// Application cache directory.
    cache_dir: PathBuf,
    /// Maximum number of isolated ebuild requests that may run concurrently.
    ebuild_jobs: NonZeroUsize,
}

impl SysConf {
    pub fn new(config_root: PathBuf, cache_dir: PathBuf) -> Self {
        Self {
            config_root,
            cache_dir,
            ebuild_jobs: thread::available_parallelism().unwrap_or(NonZeroUsize::MIN),
        }
    }

    /// Sets `ebuild_jobs`.
    pub const fn with_ebuild_jobs(mut self, jobs: NonZeroUsize) -> Self {
        self.ebuild_jobs = jobs;
        self
    }

    /// Returns the path to the repository config, usually
    /// `/etc/portage`.
    pub fn portage_conf(&self) -> PathBuf {
        self.config_root.join(PORTAGE_CONF_PATH)
    }

    /// Returns the default path to the repository config,
    /// usually `/usr/share/portage/config`.
    pub fn default_portage_conf(&self) -> PathBuf {
        self.config_root.join(DEFAULT_PORTAGE_CONF_PATH)
    }

    /// Returns the path to VDB.
    pub fn vdb_path(&self) -> PathBuf {
        self.config_root.join(VDB_PATH)
    }

    /// Returns the application cache directory.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Returns the maximum number of isolated ebuild requests
    /// that may run concurrently.
    pub const fn ebuild_jobs(&self) -> usize {
        self.ebuild_jobs.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_portage_paths() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_root = temp_dir.path().to_path_buf();
        let cache_dir = temp_dir.path().join("cache");
        let config = SysConf::new(config_root.clone(), cache_dir.clone());
        assert_eq!(config.portage_conf(), config_root.join(PORTAGE_CONF_PATH));
        assert_eq!(config.cache_dir(), cache_dir.as_path());
        assert_eq!(
            config.default_portage_conf(),
            config_root.join(DEFAULT_PORTAGE_CONF_PATH)
        );
    }
}
