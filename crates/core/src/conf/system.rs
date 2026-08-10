use std::{num::NonZeroUsize, path::PathBuf, thread};

const PORTAGE_CONF_PATH: &str = "etc/portage";
const DEFAULT_PORTAGE_CONF_PATH: &str = "usr/share/portage/config";

/// Runtime configuration shared by repository operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SysConf {
    /// Root path to configuration files, usually `/`.
    config_root: PathBuf,
    /// Maximum number of isolated ebuild requests that may run concurrently.
    ebuild_jobs: NonZeroUsize,
}

impl SysConf {
    pub fn new(config_root: PathBuf) -> Self {
        Self {
            config_root,
            ..Self::default()
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

    /// Returns the maximum number of isolated ebuild requests
    /// that may run concurrently.
    pub const fn ebuild_jobs(&self) -> usize {
        self.ebuild_jobs.get()
    }
}

impl Default for SysConf {
    fn default() -> Self {
        Self {
            config_root: PathBuf::from("/"),
            ebuild_jobs: thread::available_parallelism().unwrap_or(NonZeroUsize::MIN),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_portage_paths() {
        let temp = tempfile::tempdir().unwrap();
        let config = SysConf::new(temp.path().to_path_buf());
        assert_eq!(config.portage_conf(), temp.path().join(PORTAGE_CONF_PATH));
        assert_eq!(
            config.default_portage_conf(),
            temp.path().join(DEFAULT_PORTAGE_CONF_PATH)
        );
    }
}
