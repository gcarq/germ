mod env;
mod functions;
mod prot;
mod utils;

use crate::conf::PortageConf;
use crate::consts::{BASH_BINARY_PATH, SANDBOX_BINARY_PATH};
use crate::ebuild::Ebuild;
use crate::ebuild::handler::env::EbuildEnv;
use crate::ebuild::handler::functions::handle_request;
use crate::ebuild::handler::prot::Request;
use crate::process::Process;
use crate::repository::manager::RepoManager;
use anyhow::{Context, Result, anyhow};
use log::debug;
use nix::sys::wait::WaitStatus;

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
    env: EbuildEnv,
}

impl<'a> EbuildPhaseHandler<'a> {
    /// Create a new ebuild phase handler for the given `ebuild` and `phase`.
    pub fn new(ebuild: &'a Ebuild, conf: &'a PortageConf, phase: EbuildPhase) -> Result<Self> {
        Ok(Self {
            repo_manager: &conf.repo_manager,
            args: Self::build_args(),
            env: EbuildEnv::new(ebuild, &phase, &conf.make_env),
            ebuild,
            phase,
        })
    }

    /// Starts the process for the ebuild phase.
    /// NOTE: This call blocks until the process has been finished.
    pub fn execute(&mut self) -> Result<()> {
        debug!(
            "Executing ebuild phase '{}' for '{}' ...",
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
            let request = match ipc.recv::<Request>()? {
                Some(msg) => msg,
                // Got EOF, at this point the ebuild process should have already exited
                None => match process.wait()? {
                    WaitStatus::Exited(_, 0) => {
                        debug!("ebuild process (PID: {}) exited successfully", process.pid);
                        break;
                    }
                    WaitStatus::Exited(_, code) => {
                        return Err(anyhow!("ebuild process exited with code {code}"));
                    }
                    _ => return Err(anyhow!("ebuild process terminated abnormally")),
                },
            };
            let response = handle_request(self.ebuild, self.repo_manager, request)?;
            ipc.send(response)?;
        }
        Ok(())
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
