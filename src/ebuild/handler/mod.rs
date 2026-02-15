mod functions;
mod prot;

use crate::conf::PortageConf;
use crate::consts::{BASH_BINARY_PATH, SANDBOX_BINARY_PATH};
use crate::ebuild::Ebuild;
use crate::ebuild::handler::functions::handle_request;
use crate::ebuild::handler::prot::Request;
use crate::makenv::MakeEnv;
use crate::process::Process;
use crate::repository::manager::RepoManager;
use anyhow::{Context, Result, anyhow};
use log::debug;
use nix::sys::wait::WaitStatus;
use std::collections::HashMap;

pub enum EbuildPhase {
    Depend,
}

impl EbuildPhase {
    pub const fn as_str(&self) -> &str {
        match self {
            EbuildPhase::Depend => "depend",
        }
    }
}

/// Manages the execution of an ebuild phase.
pub struct EbuildPhaseHandler<'a> {
    ebuild: &'a Ebuild<'a>,
    repo_manager: &'a RepoManager,
    phase: EbuildPhase,
    args: Vec<String>,
    env: HashMap<String, String>,
}

impl<'a> EbuildPhaseHandler<'a> {
    /// Create a new ebuild phase handler for the given `ebuild` and `phase`.
    pub fn new(ebuild: &'a Ebuild, conf: &'a PortageConf, phase: EbuildPhase) -> Result<Self> {
        let args = Self::build_args();
        let env = Self::extend_env(ebuild, &conf.make_env, &phase)?;

        Ok(Self {
            repo_manager: &conf.repo_manager,
            ebuild,
            phase,
            args,
            env,
        })
    }

    /// Starts the process for the ebuild phase.
    /// NOTE: This call blocks until the process has been finished.
    pub fn execute(&mut self) -> Result<()> {
        debug!(
            "Executing ebuild phase '{}' for '{}'",
            self.phase.as_str(),
            self.ebuild.pkg
        );
        let mut process = Process::with_ipc(&self.args, &self.env)
            .with_context(|| "unable to spawn ebuild process")?;
        let ipc = match &mut process.ipc {
            Some(ipc) => ipc,
            None => return Err(anyhow!("IPC handler not available")),
        };

        loop {
            if !ipc.poll()? {
                continue;
            }

            let data = match ipc.recv()? {
                Some(data) => shlex::split(&data)
                    .ok_or_else(|| anyhow!("unable to split text due to syntax errors"))?,
                // Got EOF, at this point the ebuild process should have already exited
                None => match process.wait()? {
                    WaitStatus::Exited(_, 0) => break,
                    WaitStatus::Exited(_, code) => {
                        return Err(anyhow!("ebuild process exited with code {code}"));
                    }
                    _ => return Err(anyhow!("ebuild process terminated abnormally")),
                },
            };

            let data = data.iter().map(|s| s.as_str()).collect::<Vec<_>>();
            let request = Request::new(&data)?;
            let response = handle_request(self.ebuild, self.repo_manager, &request)?;
            ipc.send(&response)?;
        }
        Ok(())
    }

    /// Extends given environment variables `env` for the given `ebuild` and `phase`
    /// according to PMS 11.1.
    fn extend_env(
        ebuild: &Ebuild,
        env: &MakeEnv,
        phase: &EbuildPhase,
    ) -> Result<HashMap<String, String>> {
        let env = env
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .chain([
                (
                    "BASH_COMPAT".to_owned(),
                    ebuild.eapi.supported_bash_version()?,
                ),
                // Force invalid paths for bashrc and bash_env to avoid sourcing user files.
                ("BASHRC".to_owned(), "/dev/null".to_owned()),
                ("BASH_ENV".to_owned(), "/dev/null".to_owned()),
                ("EBUILD_PHASE".to_owned(), phase.as_str().to_owned()),
                // Ebuild variables
                ("P".to_owned(), ebuild.pkg.p()),
                ("PF".to_owned(), ebuild.pkg.pf()),
                ("PN".to_owned(), ebuild.pkg.pn()),
                ("CATEGORY".to_owned(), ebuild.pkg.category()),
                ("PV".to_owned(), ebuild.pkg.pv()),
                ("PR".to_owned(), ebuild.pkg.pr()),
                ("PVR".to_owned(), ebuild.pkg.pvr()),
                (
                    "EBUILD".to_owned(),
                    ebuild.path.to_str().unwrap().to_owned(),
                ),
            ])
            .collect::<HashMap<String, String>>();
        Ok(env)
    }

    /// Builds the list of args to be passed to bash for the ebuild process.
    /// Also sets shell options depending on the EAPI.
    /// See <https://www.gnu.org/software/bash/manual/html_node/The-Shopt-Builtin.html>
    fn build_args() -> Vec<String> {
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
            SANDBOX_BINARY_PATH,
            BASH_BINARY_PATH,
            "+O",
            "patsub_replacement",
            "+O",
            "globskipdots",
            "-O",
            "failglob",
        ];
        args.extend_from_slice(&["-c", "./bin/ebuild.sh"]);
        args.into_iter().map(|arg| arg.to_owned()).collect()
    }
}
