mod env;
mod exec;
mod functions;
mod ipc;
mod prot;

use crate::consts::{BASH_BINARY_PATH, SANDBOX_BINARY_PATH};
use crate::ebuild::Ebuild;
use crate::ebuild::handler::env::EbuildEnv;
use crate::ebuild::handler::functions::version::{ver_cut, ver_rs, ver_test};
use crate::ebuild::handler::functions::{debug_print, die, resolve_eclass};
use crate::ebuild::handler::prot::func::{FuncCall, FuncType};
use crate::ebuild::handler::prot::{ChildMessage, ParentMessage};
use crate::makenv::MakeEnv;
use anyhow::{Result, bail};
use exec::EbuildExecution;
use ipc::IpcHandler;
use log::debug;
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
    pub fn new(ebuild: &'a Ebuild, phase: EbuildPhase, make_env: &MakeEnv) -> Result<Self> {
        Ok(Self {
            env: EbuildEnv::new(ebuild, &phase, make_env)?,
            ebuild,
            phase,
        })
    }

    /// Spawns the process and returns the data sent by the ebuild process.
    /// NOTE: This call blocks until the process has been finished or the
    /// IPC channel has been closed.
    pub fn spawn(&mut self) -> Result<Vec<String>> {
        debug!(
            "Executing ebuild phase '{}' for '{}' ...",
            self.phase, self.ebuild.cpv
        );

        let args = Self::build_args();
        let (ipc, child) = IpcHandler::spawn(SANDBOX_BINARY_PATH, &args, &self.env)?;
        let mut execution = EbuildExecution::new(ipc, child);
        execution.run(|channel| self.handle_messages(channel))
    }

    fn handle_messages(&self, ipc: &mut IpcHandler) -> Result<Vec<String>> {
        let mut data = Vec::new();
        while let Some(request) = ipc.recv::<ChildMessage>()? {
            match request {
                ChildMessage::Call(func_call) => {
                    let response = self.handle_request(func_call)?;
                    let died = matches!(response, ParentMessage::Die);
                    ipc.send(response)?;
                    if died {
                        return Ok(Vec::new());
                    }
                }
                ChildMessage::Data(value) => data.push(value),
            }
        }
        Ok(data)
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
                _ => bail!("invalid arguments: __resolve_eclass <name>: {args:?}"),
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
                _ => bail!("invalid arguments: has <word> <args>: {args:?}"),
            },
            FuncType::HasV if self.ebuild.eapi.supports_hasv() => match args {
                [word, args @ ..] => match args.contains(word) {
                    true => Ok(ParentMessage::Ok(Some(word.clone()))),
                    false => Ok(ParentMessage::Err(None)),
                },
                _ => bail!("invalid arguments: hasv <word> <args>: {args:?}"),
            },
            FuncType::HasQ if self.ebuild.eapi.supports_hasq() => match args {
                [word, args @ ..] => match args.contains(word) {
                    true => Ok(ParentMessage::Ok(None)),
                    false => Ok(ParentMessage::Err(None)),
                },
                _ => bail!("invalid arguments: hasq <word> <args>: {args:?}"),
            },
            FuncType::VerCut => match args {
                [range] => ver_cut(self.ebuild.cpv, range, None),
                [range, version] => ver_cut(self.ebuild.cpv, range, Some(version)),
                _ => bail!("invalid arguments: ver_cut <range> [<version>]: {args:?}"),
            },
            FuncType::VerRs => ver_rs(self.ebuild.cpv, args),
            FuncType::VerTest => match args {
                [op, v2] => ver_test(self.ebuild.cpv, None, op, v2),
                [v1, op, v2] => ver_test(self.ebuild.cpv, Some(v1), op, v2),
                _ => bail!("invalid arguments: ver_test [<v1>] op <v2>: {args:?}"),
            },
            FuncType::HasV | FuncType::HasQ => bail!(
                "unsupported function '{}' for EAPI '{}'",
                call.func,
                self.ebuild.eapi,
            ),
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
    use crate::ebuild::handler::ipc::ChildToParentMsg;
    use crate::types::FxHashMap;
    use anyhow::anyhow;

    struct Ready;

    impl ChildToParentMsg for Ready {
        fn from_bytes(_bytes: &[u8]) -> Result<Self> {
            Ok(Self)
        }
    }

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

        let result: Result<()> = execution.run(|channel| {
            channel
                .recv::<Ready>()?
                .ok_or_else(|| anyhow!("unexpected EOF"))?;
            Err(anyhow!("protocol failure"))
        });

        assert!(
            result.is_err()
                && execution
                    .exit_status()
                    .is_some_and(|status| status.success())
        );
    }
}
