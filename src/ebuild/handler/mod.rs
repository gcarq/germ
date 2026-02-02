mod functions;

use crate::consts::BASH_BINARY_PATH;
use crate::ebuild::Ebuild;
use crate::ebuild::handler::functions::exec_ebuild_fn;
use crate::package::Package;
use crate::process::Process;
use anyhow::{Context, Result, anyhow};
use nix::sys::wait::WaitStatus;
use std::collections::HashMap;

pub enum EbuildPhase {
    Metadata,
}

/// Manages the execution of an ebuild phase.
pub struct EbuildPhaseHandler<'a> {
    package: &'a Package,
    phase: EbuildPhase,
    args: Vec<String>,
    env: HashMap<String, String>,
    //process: Option<Process>,
}

impl<'a> EbuildPhaseHandler<'a> {
    /// Create a new ebuild phase handler for the given `package` and `phase`.
    pub fn new(package: &'a Package, phase: EbuildPhase) -> Result<Self> {
        if package.ebuild.is_none() {
            return Err(anyhow!("no ebuild found for package: {package}"));
        }
        // Safe to unwrap as we check for ebuild presence above
        let args = Self::build_args(package.ebuild.as_ref().unwrap());
        let env = Self::create_ebuild_env(package)?;

        Ok(Self {
            package,
            phase,
            args,
            env,
            //process: None,
        })
    }

    /// Starts the process for the ebuild phase.
    /// NOTE: This call blocks until the process has been finished.
    pub fn execute(&mut self) -> Result<()> {
        let mut process = Process::new(&self.args, &self.env)
            .with_context(|| "unable to spawn ebuild process")?;
        //self.process = Some(process);

        loop {
            if process.ipc.poll()? {
                let data = match process.ipc.recv()? {
                    Some(data) => data,
                    // Got EOF, at this point the ebuild process should have already exited
                    None => match process.wait()? {
                        WaitStatus::Exited(_, code) => match code {
                            0 => break,
                            _ => return Err(anyhow!("ebuild process exited with code {code}")),
                        },
                        _ => return Err(anyhow!("ebuild process terminated abnormally")),
                    },
                };

                let result = self.handle_request(&data)?;
                process.ipc.send(&result)?;
            }
        }

        Ok(())
    }

    /// Transforms the given response `data` received from the ebuild process
    /// and returns the string that should be sent back.
    fn handle_request(&self, data: &str) -> Result<String> {
        let parts = data.split_ascii_whitespace().collect::<Box<[&str]>>();
        match parts.as_ref() {
            ["FN", fn_name, args @ ..] => exec_ebuild_fn(self.package, fn_name, args),
            _ => Err(anyhow!("unknown ebuild IPC request: {data}")),
        }
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
    fn build_args(ebuild: &Ebuild) -> Vec<String> {
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
        args.into_iter().map(|arg| arg.to_owned()).collect()
    }
}
