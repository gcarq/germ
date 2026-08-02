use anyhow::Context;

use crate::ebuild::Ebuild;
use crate::ebuild::handler::EbuildPhase;
use crate::makenv::MakeEnv;
use crate::types::FxHashMap;
use log::warn;
use std::ops::Deref;

/// Unset selected variables so that they don't needlessly propagate down into the ebuild
/// environment.
///
/// Exclude anything that is not allowed (like `BZIP`) or extremely long (like `SRC_URI`)
/// since that could cause `execve()` calls to fail with E2BIG errors.
const ENV_UNSET: [&str; 88] = [
    // Must be unset to avoid breaking the ebuild process
    "GZIP",
    "BZIP",
    "BZIP2",
    "CDPATH",
    "GREP_OPTIONS",
    "GREP_COLOR",
    "GLOBIGNORE",
    // Variables that are set by the ebuild
    "DEPEND",
    "RDEPEND",
    "PDEPEND",
    "SRC_URI",
    "BDEPEND",
    "IDEPEND",
    // Misc variables inherited from the calling environment
    "INFOPATH",
    "MANPATH",
    "USER",
    // Variables that break bash
    "GLOBSORT",
    "HISTFILE",
    "POSIXLY_CORRECT",
    // Portage config variables and variables set directly by portage
    "ACCEPT_CHOSTS",
    "ACCEPT_KEYWORDS",
    "ACCEPT_PROPERTIES",
    "ACCEPT_RESTRICT",
    "AUTOCLEAN",
    "BINPKG_COMPRESS",
    "BINPKG_COMPRESS_FLAGS",
    "CLEAN_DELAY",
    "COLLISION_IGNORE",
    "CONFIG_PROTECT",
    "CONFIG_PROTECT_MASK",
    "EGENCACHE_DEFAULT_OPTS",
    "EMERGE_DEFAULT_OPTS",
    "EMERGE_LOG_DIR",
    "EMERGE_WARNING_DELAY",
    "FETCHCOMMAND",
    "FETCHCOMMAND_FTP",
    "FETCHCOMMAND_HTTP",
    "FETCHCOMMAND_HTTPS",
    "FETCHCOMMAND_RSYNC",
    "FETCHCOMMAND_SFTP",
    "FETCHCOMMAND_SSH",
    "GENTOO_MIRRORS",
    "NOCONFMEM",
    "O",
    "PORTAGE_BACKGROUND",
    "PORTAGE_BACKGROUND_UNMERGE",
    "PORTAGE_BINHOST",
    "PORTAGE_BINPKG_FORMAT",
    "PORTAGE_BUILDDIR_LOCKED",
    "PORTAGE_CHECKSUM_FILTER",
    "PORTAGE_ELOG_CLASSES",
    "PORTAGE_ELOG_MAILFROM",
    "PORTAGE_ELOG_MAILSUBJECT",
    "PORTAGE_ELOG_MAILURI",
    "PORTAGE_ELOG_SYSTEM",
    "PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS",
    "PORTAGE_FETCH_RESUME_MIN_SIZE",
    "PORTAGE_GPG_DIR",
    "PORTAGE_GPG_KEY",
    "PORTAGE_GPG_SIGNING_COMMAND",
    "PORTAGE_IONICE_COMMAND",
    "PORTAGE_PACKAGE_EMPTY_ABORT",
    "PORTAGE_REPO_DUPLICATE_WARN",
    "PORTAGE_RO_DISTDIRS",
    "PORTAGE_RSYNC_EXTRA_OPTS",
    "PORTAGE_RSYNC_OPTS",
    "PORTAGE_RSYNC_RETRIES",
    "PORTAGE_SSH_OPTS",
    "PORTAGE_SYNC_STALE",
    "PORTAGE_TRUST_HELPER",
    "PORTAGE_USE",
    "PORTAGE_LOG_FILTER_FILE_CMD",
    "PORTAGE_LOGDIR",
    "PORTAGE_LOGDIR_CLEAN",
    "QUICKPKG_DEFAULT_OPTS",
    "REPOMAN_DEFAULT_OPTS",
    "RESUMECOMMAND",
    "RESUMECOMMAND_FTP",
    "RESUMECOMMAND_HTTP",
    "RESUMECOMMAND_HTTPS",
    "RESUMECOMMAND_RSYNC",
    "RESUMECOMMAND_SFTP",
    "RESUMECOMMAND_SSH",
    "UNINSTALL_IGNORE",
    "USE_EXPAND_HIDDEN",
    "USE_ORDER",
    "__PORTAGE_HELPER",
    // No longer supported variables
    "SYNC",
];

/// Internal variables that are set by this process and are not inherited from config files.
const ENV_INTERNALS: [&str; 51] = [
    "A",
    "AA",
    "BASH_FUNC____in_portage_iuse%%",
    "BDEPEND",
    "BROOT",
    "CATEGORY",
    "DEPEND",
    "DESCRIPTION",
    "DOCS",
    "EAPI",
    "EBUILD_FORCE_TEST",
    "EBUILD_PHASE",
    "EBUILD_PHASE_FUNC",
    "EBUILD_SKIP_MANIFEST",
    "ED",
    "EMERGE_FROM",
    "EPREFIX",
    "EROOT",
    "HOMEPAGE",
    "IDEPEND",
    "INHERITED",
    "IUSE",
    "IUSE_EFFECTIVE",
    "KEYWORDS",
    "LICENSE",
    "MERGE_TYPE",
    "PDEPEND",
    "PF",
    "PKGUSE",
    "PORTAGE_BACKGROUND",
    "PORTAGE_BACKGROUND_UNMERGE",
    "PORTAGE_BUILDDIR_LOCKED",
    "PORTAGE_BUILT_USE",
    "PORTAGE_CONFIGROOT",
    "PORTAGE_EXPLICIT_INHERIT",
    "PORTAGE_INTERNAL_CALLER",
    "PORTAGE_IUSE",
    "PORTAGE_NONFATAL",
    "PORTAGE_PIPE_FD",
    "PORTAGE_REPO_NAME",
    "PORTAGE_USE",
    "PROPERTIES",
    "RDEPEND",
    "REPOSITORY",
    "REQUIRED_USE",
    "RESTRICT",
    "ROOT",
    "SANDBOX_LOG",
    "SLOT",
    "SRC_URI",
    "_",
];

/// Holds all environment variables for an ebuild process.
///
/// All variable names listed in `ENV_UNSET` and `make_env["ENV_UNSET"]` will be removed.
pub struct EbuildEnv(FxHashMap<String, String>);

impl EbuildEnv {
    /// Builds the ebuild environment that can be passed to the ebuild process.
    ///
    /// TODO: ensure `LC_CTYPE` and `LC_COLLATE` are equivalent to POSIX locale
    /// TODO: add more variables as needed
    pub fn new(ebuild: &Ebuild, phase: &EbuildPhase, make_env: &MakeEnv) -> anyhow::Result<Self> {
        let repo_paths = shlex::try_join(
            ebuild
                .repo
                .eclasses()?
                .repo_paths()
                .iter()
                .filter_map(|p| p.as_os_str().to_str()),
        )
        .with_context(|| "unable to escape repo paths")?;

        let bash_version = ebuild.eapi.supported_bash_version().to_owned();

        let mut env = make_env
            .iter()
            .filter_map(|(name, value)| {
                Self::filter_var(name).then_some((name.clone(), value.to_string()))
            })
            .chain([
                ("PORTAGE_ECLASS_LOCATIONS".to_owned(), repo_paths),
                ("BASH_COMPAT".to_owned(), bash_version),
                // Force invalid paths for bashrc and bash_env to avoid sourcing user files.
                ("BASHRC".to_owned(), "/dev/null".to_owned()),
                ("BASH_ENV".to_owned(), "/dev/null".to_owned()),
                // TODO: add support for enabling debug mode
                ("EBUILD_DEBUG".to_owned(), "0".to_owned()),
                // Ebuild variables, see PMS 11.1
                ("P".to_owned(), ebuild.cpv.p()),
                ("PF".to_owned(), ebuild.cpv.pf()),
                ("PN".to_owned(), ebuild.cpv.pn().to_owned()),
                ("CATEGORY".to_owned(), ebuild.cpv.category().to_owned()),
                ("PV".to_owned(), ebuild.cpv.pv()),
                ("PR".to_owned(), ebuild.cpv.pr()),
                ("PVR".to_owned(), ebuild.cpv.pvr()),
                (
                    "EBUILD".to_owned(),
                    ebuild.path.to_str().unwrap().to_owned(),
                ),
                ("EBUILD_PHASE".to_owned(), phase.to_string()),
            ])
            .collect::<FxHashMap<String, String>>();

        if let Some(env_unset) = make_env.get("ENV_UNSET") {
            for name in env_unset.inner() {
                env.remove(name);
            }
        }
        Ok(Self(env))
    }

    /// Returns true if the given variable `name` is allowed to be propagated into the ebuild env.
    fn filter_var(name: &str) -> bool {
        if name.starts_with("__") || ENV_UNSET.contains(&name) {
            return false;
        }
        if ENV_INTERNALS.contains(&name) {
            warn!("Ignoring '{name}' because it's reserved for internal use");
            return false;
        }
        true
    }
}

impl Deref for EbuildEnv {
    type Target = FxHashMap<String, String>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_var() {
        assert!(!EbuildEnv::filter_var("GZIP"));
        assert!(!EbuildEnv::filter_var("DEPEND"));
        assert!(!EbuildEnv::filter_var("USER"));
        assert!(!EbuildEnv::filter_var("BASH_FUNC____in_portage_iuse%%"));
        assert!(EbuildEnv::filter_var("LC_ALL"));
    }
}
