mod env;
pub mod error;
mod exec;
mod functions;
mod ipc;
mod protocol;

use crate::consts::{BASH_BINARY_PATH, SANDBOX_BINARY_PATH};
use crate::ebuild::Ebuild;
use crate::makenv::MakeEnv;
use anyhow::anyhow;

use env::EbuildEnv;
use error::{FuncCallError, PhaseExecutionError};
use exec::EbuildExecution;
use functions::version::{ver_cut, ver_rs, ver_test};
use functions::{debug_print, die, resolve_eclass};
use ipc::IpcHandler;
use log::debug;
use protocol::{EbuildMessage, FuncCall, FuncType, FunctionReply};
use std::fmt;

/// Defines all implemented ebuild phases.
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

/// Maps an invalid function call to a [`FuncCallError::InvalidArgs`] error.
fn map_invalid_args(
    func: FuncType,
    args: Vec<String>,
    result: anyhow::Result<FunctionReply>,
) -> Result<FunctionReply, PhaseExecutionError> {
    result.map_err(|source| FuncCallError::InvalidArgs { func, args, source }.into())
}

/// Manages the execution of an ebuild phase.
pub struct EbuildPhaseHandler<'a> {
    ebuild: &'a Ebuild<'a>,
    phase: EbuildPhase,
    env: EbuildEnv,
}

impl<'a> EbuildPhaseHandler<'a> {
    /// Create a new ebuild phase handler for the given `ebuild` and `phase`.
    pub fn new(ebuild: &'a Ebuild, phase: EbuildPhase, make_env: &MakeEnv) -> anyhow::Result<Self> {
        Ok(Self {
            env: EbuildEnv::new(ebuild, &phase, make_env)?,
            ebuild,
            phase,
        })
    }

    /// Spawns the process and returns the data sent by the ebuild process.
    /// NOTE: This call blocks until the process has been finished or the
    /// IPC channel has been closed.
    pub fn spawn(&mut self) -> Result<Vec<String>, PhaseExecutionError> {
        debug!(
            "Executing ebuild phase '{}' for '{}' ...",
            self.phase, self.ebuild.cpv
        );

        let args = Self::build_args();
        let (ipc, child) = IpcHandler::spawn(SANDBOX_BINARY_PATH, &args, &self.env)?;
        let mut execution = EbuildExecution::new(ipc, child);
        execution.run(|channel| self.handle_messages(channel))
    }

    fn handle_messages(&self, ipc: &mut IpcHandler) -> Result<Vec<String>, PhaseExecutionError> {
        let mut data = Vec::new();
        while let Some(bytes) = ipc.recv_bytes()? {
            match EbuildMessage::from_bytes(bytes)? {
                EbuildMessage::Call(func_call) => {
                    let response = self.handle_request(func_call)?;
                    let die_message = match &response {
                        FunctionReply::Die(message) => Some(message.clone()),
                        _ => None,
                    };
                    ipc.send(&response.into_bytes())?;
                    if let Some(message) = die_message {
                        return Err(PhaseExecutionError::Die(message));
                    }
                }
                EbuildMessage::Data(value) => data.push(value),
            }
        }
        Ok(data)
    }

    /// Executes a function for the given [`FuncCall`].
    ///
    /// Returns a [`FunctionReply`] with the result of the function or an `Err` if the request
    /// is invalid or the function execution fails.
    fn handle_request(&self, call: FuncCall) -> Result<FunctionReply, PhaseExecutionError> {
        let FuncCall { func, args } = call;
        match func {
            FuncType::ResolveEclass => match args.as_slice() {
                [name] => Ok(resolve_eclass(name, self.ebuild.repo)?),
                _ => map_invalid_args(func, args, Err(anyhow!("expected one argument"))),
            },
            FuncType::DebugPrint => Ok(debug_print(&args)),
            FuncType::Die => match args.as_slice() {
                [first, args @ ..] if first == "-n" => Ok(die(args, false)),
                args => Ok(die(args, true)),
            },
            FuncType::Has => {
                let result = match args.as_slice() {
                    [word, args @ ..] if args.contains(word) => Ok(FunctionReply::Ok(None)),
                    [..] if !args.is_empty() => Ok(FunctionReply::Err(None)),
                    _ => Err(anyhow!("expected a word and one or more values")),
                };
                map_invalid_args(func, args, result)
            }
            FuncType::HasV if self.ebuild.eapi.supports_hasv() => {
                let result = match args.as_slice() {
                    [word, args @ ..] if args.contains(word) => {
                        Ok(FunctionReply::Ok(Some(word.clone())))
                    }
                    [..] if !args.is_empty() => Ok(FunctionReply::Err(None)),
                    _ => Err(anyhow!("expected a word and one or more values")),
                };
                map_invalid_args(func, args, result)
            }
            FuncType::HasQ if self.ebuild.eapi.supports_hasq() => {
                let result = match args.as_slice() {
                    [word, args @ ..] if args.contains(word) => Ok(FunctionReply::Ok(None)),
                    [..] if !args.is_empty() => Ok(FunctionReply::Err(None)),
                    _ => Err(anyhow!("expected a word and one or more values")),
                };
                map_invalid_args(func, args, result)
            }
            FuncType::VerCut => {
                let result = match args.as_slice() {
                    [range] => ver_cut(self.ebuild.cpv, range, None),
                    [range, version] => ver_cut(self.ebuild.cpv, range, Some(version)),
                    _ => Err(anyhow!("expected a range and optional version")),
                };
                map_invalid_args(func, args, result)
            }
            FuncType::VerRs => {
                let result = ver_rs(self.ebuild.cpv, &args);
                map_invalid_args(func, args, result)
            }
            FuncType::VerTest => {
                let result = match args.as_slice() {
                    [op, v2] => ver_test(self.ebuild.cpv, None, op, v2),
                    [v1, op, v2] => ver_test(self.ebuild.cpv, Some(v1), op, v2),
                    _ => Err(anyhow!(
                        "expected an optional version, operator, and version"
                    )),
                };
                map_invalid_args(func, args, result)
            }
            FuncType::HasV | FuncType::HasQ => Err(FuncCallError::Unsupported {
                func,
                eapi: self.ebuild.eapi.clone(),
            }
            .into()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ebuild::handler::error::ProtocolError, types::FxHashMap};

    #[test]
    fn test_abort_ipc_ordering() {
        let args = vec![
            "-c".into(),
            r#"
                trap 'if IFS= read -r <&${CHILD_READ_FD}; then exit 2; else exit 0; fi' TERM
                printf 'ready\4' >&${CHILD_WRITE_FD} || exit 3
                while :; do :; done
            "#
            .into(),
        ];
        let (ipc, child) =
            IpcHandler::spawn(BASH_BINARY_PATH, &args, &FxHashMap::default()).unwrap();
        let mut execution = EbuildExecution::new(ipc, child);

        let result: Result<(), PhaseExecutionError> = execution.run(|channel| {
            channel
                .recv_bytes()?
                .ok_or_else(|| ProtocolError::InvalidRequest("".into()))?;
            Err(PhaseExecutionError::Invariant(anyhow!("test")))
        });

        assert!(
            result.is_err()
                && execution
                    .exit_status()
                    .is_some_and(|status| status.success())
        );
    }
}
