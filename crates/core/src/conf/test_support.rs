use tempfile::TempDir;

use super::system::SysConf;

/// Builds a [`SysConf`] with paths rooted in `temp_dir`.
pub fn build_sysconf(temp_dir: &TempDir) -> SysConf {
    SysConf::new(temp_dir.path().to_owned(), temp_dir.path().join(".cache"))
}
