use crate::consts::BASH_BINARY_PATH;
use crate::package::Package;
use crate::package::ebuild::Ebuild;
use crate::process::Process;
use anyhow::{Context, Result, anyhow};
use nix::sys::wait::WaitStatus;
use std::collections::HashMap;

pub enum EbuildPhase {
    Metadata,
}

/// Manages the execution of an ebuild phase.
pub struct EbuildProcess<'a> {
    package: &'a Package,
    phase: EbuildPhase,
    process: Process,
}

impl<'a> EbuildProcess<'a> {
    /// Spawns a new ebuild process for the given `package` and `phase`.
    pub fn new(package: &'a Package, phase: EbuildPhase) -> Result<Self> {
        if package.ebuild.is_none() {
            return Err(anyhow!("no ebuild found for package: {package}"));
        }
        // Safe to unwrap as we check for ebuild presence above
        let args = Self::build_args(package.ebuild.as_ref().unwrap());
        let env = Self::create_ebuild_env(package)?;
        let process = Process::new(&args, env).with_context(|| "unable to spawn ebuild process")?;

        Ok(Self {
            package,
            phase,
            process,
        })
    }

    /// Waits for the ebuild process to finish and returns its exit status.
    pub fn wait(&mut self) -> Result<WaitStatus> {
        self.process.wait()
    }

    /// Creates environment variables for the given `package` according to PMS section 11.1.
    /// The caller must ensure the `package` has an associated ebuild set.
    fn create_ebuild_env(package: &Package) -> Result<HashMap<String, String>> {
        // Safe to unwrap as we check for ebuild presence in `new()`
        let ebuild = package.ebuild.as_ref().unwrap();
        let env = vec![
            (
                "BASH_COMPAT".to_owned(),
                ebuild.eapi.supported_bash_version()?,
            ),
            // Force invalid paths for bashrc and bash_env to avoid sourcing user files.
            ("BASHRC".to_owned(), "/dev/null".to_owned()),
            ("BASH_ENV".to_owned(), "/dev/null".to_owned()),
            // Ebuild variables
            ("P".to_owned(), package.p()),
            ("PF".to_owned(), package.pf()),
            ("PN".to_owned(), package.pn()),
            ("CATEGORY".to_owned(), package.category()),
            ("PV".to_owned(), package.pv()),
            ("PR".to_owned(), package.pr()),
            ("PVR".to_owned(), package.pvr()),
            (
                "EBUILD".to_owned(),
                ebuild.path.to_str().unwrap().to_owned(),
            ),
        ];
        Ok(HashMap::from_iter(env))
    }

    /// Builds the list of `args` to be passed to bash for the ebuild process.
    /// Also sets shell options depending on the EAPI.
    /// See https://www.gnu.org/software/bash/manual/html_node/The-Shopt-Builtin.html
    fn build_args(ebuild: &Ebuild) -> Vec<&str> {
        // The "patsub_replacement" and "globskipdots" options were introduced
        // by bash-5.2. Both are default-enabled and change the behavior of
        // bash in a manner that is backwards-incompatible. Setting BASH_COMPAT
        // has no effect on either option. Hence, ensure that both are disabled
        // until such time as a future EAPI not only requires >=5.2, but also
        // mandates that the options be enabled.
        //
        // https://bugs.gentoo.org/881383
        // https://bugs.gentoo.org/907061
        // https://bugs.gentoo.org/946193
        // https://bugs.gentoo.org/946179
        let mut args = vec![
            BASH_BINARY_PATH,
            "+O",
            "patsub_replacement",
            "+O",
            "globskipdots",
        ];
        if ebuild.eapi.enables_failglob() {
            args.extend(vec!["-O", "failglob"]);
        }
        args.extend_from_slice(&["-c", "./bin/ebuild.sh"]);
        args
    }
}
