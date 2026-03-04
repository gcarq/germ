mod env;
mod functions;
mod prot;

use crate::consts::{BASH_BINARY_PATH, SANDBOX_BINARY_PATH};
use crate::ebuild::Ebuild;
use crate::ebuild::handler::env::EbuildEnv;
use crate::ebuild::handler::functions::handle_request;
use crate::ebuild::handler::prot::ChildMessage;
use crate::makenv::MakeEnv;
use crate::process::Process;
use anyhow::{Context, Result, anyhow};
use log::debug;
use nix::sys::wait::WaitStatus;
use std::fmt;

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

impl fmt::Display for EbuildPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Manages the execution of an ebuild phase.
pub struct EbuildPhaseHandler<'a> {
    ebuild: &'a Ebuild<'a>,
    phase: EbuildPhase,
    args: Vec<String>,
    env: EbuildEnv,
}

impl<'a> EbuildPhaseHandler<'a> {
    /// Create a new ebuild phase handler for the given `ebuild` and `phase`.
    pub fn new(ebuild: &'a Ebuild, phase: EbuildPhase, make_env: &MakeEnv) -> Self {
        Self {
            args: Self::build_args(),
            env: EbuildEnv::new(ebuild, &phase, make_env),
            ebuild,
            phase,
        }
    }

    /// Spawns the process and returns the data sent by the ebuild process.
    /// NOTE: This call blocks until the process has been finished or the
    /// IPC channel has been closed.
    pub fn spawn(&mut self) -> Result<Vec<String>> {
        debug!(
            "Executing ebuild phase '{}' for '{}' ...",
            self.phase, self.ebuild.pkg
        );

        let mut received_data = Vec::new();
        // TODO: create constants
        let child_channel = (10, 11);

        self.env.add_ipc_channel(child_channel);
        let mut process = Process::with_ipc(&self.args, &self.env, child_channel)
            .with_context(|| "unable to spawn ebuild process")?;
        let Some(ipc) = &mut process.ipc else {
            return Err(anyhow!("IPC handler not available"));
        };

        loop {
            let Some(request) = ipc.recv::<ChildMessage>()? else {
                // Got EOF, at this point the ebuild process closed its IPC channel.
                // Wait for the process to exit and check its status.
                match process.wait()? {
                    WaitStatus::Exited(_, 0) => {
                        debug!("ebuild process (PID: {}) exited successfully", process.pid);
                        break;
                    }
                    WaitStatus::Exited(_, code) => Err(anyhow!("ebuild process exited: {code}"))?,
                    _ => Err(anyhow!("ebuild process terminated abnormally"))?,
                }
            };

            match request {
                ChildMessage::Call(func_call) => {
                    let response = handle_request(self.ebuild, func_call)?;
                    ipc.send(response)?;
                }
                ChildMessage::Data(data) => received_data.push(data),
            }
        }
        Ok(received_data)
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
        args.into_iter().map(ToOwned::to_owned).collect()
    }
}
