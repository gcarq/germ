mod env;
mod functions;
mod ipc;
mod prot;

use crate::consts::{BASH_BINARY_PATH, SANDBOX_BINARY_PATH};
use crate::ebuild::Ebuild;
use crate::ebuild::handler::env::EbuildEnv;
use crate::ebuild::handler::functions::version::{ver_cut, ver_rs, ver_test};
use crate::ebuild::handler::functions::{contains_word, debug_print, die, resolve_eclass};
use crate::ebuild::handler::prot::func::{FuncCall, FuncType};
use crate::ebuild::handler::prot::{ChildMessage, ParentMessage};
use crate::makenv::MakeEnv;
use anyhow::{Result, anyhow};
use ipc::IpcHandler;
use log::{debug, trace};
use std::fmt;
use std::ops::Deref;

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
        f.write_str(self.as_str())
    }
}

/// Manages the execution of an ebuild phase.
pub struct EbuildPhaseHandler<'a> {
    ebuild: &'a Ebuild<'a>,
    phase: EbuildPhase,
    env: EbuildEnv,
}

impl<'a> EbuildPhaseHandler<'a> {
    /// Create a new ebuild phase handler for the given `ebuild` and `phase`.
    pub fn new(ebuild: &'a Ebuild, phase: EbuildPhase, make_env: &MakeEnv) -> Self {
        Self {
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
            self.phase, self.ebuild.cpv
        );

        let mut received_data = Vec::new();

        let args = Self::build_args();
        let (mut ipc, mut process) = IpcHandler::spawn(SANDBOX_BINARY_PATH, &args, &self.env)?;
        loop {
            let Some(request) = ipc.recv::<ChildMessage>()? else {
                // Got EOF, at this point the ebuild process closed its IPC channel.
                // Wait for the process to exit and check its status.
                let status = process.wait()?;
                if status.success() {
                    trace!("ebuild process (PID: {}) exited successfully", process.id());
                    break;
                }
                return Err(anyhow!(
                    "ebuild process exited with non-zero status: {status}"
                ));
            };

            match request {
                ChildMessage::Call(func_call) => {
                    let response = self.handle_request(func_call)?;
                    ipc.send(response)?;
                }
                ChildMessage::Data(data) => received_data.push(data),
            }
        }
        Ok(received_data)
    }

    /// Executes a function for the given [`FuncCall`].
    ///
    /// Returns a [`ParentMessage`] with the result of the function or an `Err` if the request
    /// is invalid or the function execution fails.
    fn handle_request(&self, call: FuncCall) -> Result<ParentMessage> {
        let args = call.args.deref();
        match call.func {
            FuncType::ResolveEclass => match args {
                [name] => resolve_eclass(name, self.ebuild.repo),
                _ => Err(anyhow!(
                    "invalid arguments: __resolve_eclass <name>: {args:?}",
                )),
            },
            FuncType::ContainsWord => match args {
                [word, args @ ..] => Ok(contains_word(word, args)),
                _ => Err(anyhow!(
                    "invalid arguments: contains_word <word> <string>: {args:?}",
                )),
            },
            FuncType::DebugPrint => Ok(debug_print(args)),
            FuncType::Die => match args {
                [first, args @ ..] if first == "-n" => Ok(die(args, false)),
                args => Ok(die(args, true)),
            },
            FuncType::Has => match args {
                [word, args @ ..] => match args.contains(word) {
                    true => Ok(ParentMessage::Ok(None)),
                    false => Ok(ParentMessage::Err(None)),
                },
                _ => Err(anyhow!("invalid arguments: has <word> <args>: {args:?}",)),
            },
            FuncType::HasV if self.ebuild.eapi.supports_hasv() => match args {
                [word, args @ ..] => match args.contains(word) {
                    true => Ok(ParentMessage::Ok(Some(word.clone()))),
                    false => Ok(ParentMessage::Err(None)),
                },
                _ => Err(anyhow!("invalid arguments: hasv <word> <args>: {args:?}",)),
            },
            FuncType::HasQ if self.ebuild.eapi.supports_hasq() => match args {
                [word, args @ ..] => match args.contains(word) {
                    true => Ok(ParentMessage::Ok(None)),
                    false => Ok(ParentMessage::Err(None)),
                },
                _ => Err(anyhow!("invalid arguments: hasq <word> <args>: {args:?}",)),
            },
            FuncType::VerCut => match args {
                [range] => ver_cut(self.ebuild.cpv, range, None),
                [range, version] => ver_cut(self.ebuild.cpv, range, Some(version)),
                _ => Err(anyhow!(
                    "invalid arguments: ver_cut <range> [<version>]: {args:?}",
                )),
            },
            FuncType::VerRs => ver_rs(self.ebuild.cpv, args),
            FuncType::VerTest => match args {
                [op, v2] => ver_test(self.ebuild.cpv, None, op, v2),
                [v1, op, v2] => ver_test(self.ebuild.cpv, Some(v1), op, v2),
                _ => Err(anyhow!(
                    "invalid arguments: ver_test [<v1>] op <v2>: {args:?}",
                )),
            },
            FuncType::HasV | FuncType::HasQ => Err(anyhow!(
                "unsupported function '{}' for EAPI '{}'",
                call.func,
                self.ebuild.eapi,
            )),
        }
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
