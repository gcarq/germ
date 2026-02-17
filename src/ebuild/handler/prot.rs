//! This module defines the protocol for the IPC between this and the ebuild process.
use crate::ebuild::handler::utils;
use crate::process::ipc::{ChildMessage, ParentMessage};
use anyhow::{Context, Result, anyhow};
use log::trace;
use shlex::Shlex;
use std::fmt;
use std::str::FromStr;
use strum::{Display, EnumString};

/// All supported ebuild functions that can be called from the ebuild process.
#[derive(EnumString, Display, PartialEq, Debug)]
pub enum FuncType {
    // Internal ebuild functions
    #[strum(serialize = "__resolve_eclass")]
    ResolveEclass,

    // Misc Commands
    #[strum(serialize = "contains_word")]
    ContainsWord,
    #[strum(serialize = "debug-print")]
    DebugPrint,
    #[strum(serialize = "die")]
    Die,

    // PMS 12.3.13 text list functions
    #[strum(serialize = "has")]
    Has,
    #[strum(serialize = "hasv")]
    HasV,
    #[strum(serialize = "hasq")]
    HasQ,

    // PMS 12.3.14 version functions
    #[strum(serialize = "ver_cut")]
    VerCut,
    #[strum(serialize = "ver_rs")]
    VerRs,
    #[strum(serialize = "ver_test")]
    VerTest,
}

/// Represents a request from the ebuild process to execute a function with the given arguments.
pub struct Request {
    pub func: FuncType,
    pub args: Vec<String>,
}

impl ChildMessage for Request {
    fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let data = str::from_utf8(&bytes).with_context(|| "invalid UTF-8 in request message")?;
        let data = utils::unescape(data);
        let mut shlex = Shlex::new(&data);
        if shlex.had_error {
            return Err(anyhow!("unable to split text due to syntax errors: {data}"));
        }

        match shlex.next().as_deref() {
            Some("FN") => {}
            _ => return Err(anyhow!("invalid ebuild IPC request: {data}")),
        }

        let func = match shlex.next() {
            Some(func_name) => FuncType::from_str(&func_name)
                .with_context(|| anyhow!("unable to resolve func for '{func_name}'"))?,
            None => return Err(anyhow!("invalid ebuild IPC request: {data}")),
        };
        let args = shlex.collect::<Vec<_>>();
        trace!("Parsed request: func='{func}', args={}", args.join(" "));
        Ok(Self { func, args })
    }
}

impl fmt::Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.func, self.args.join(" "))
    }
}

/// Represents a response to be sent back to the ebuild process after handling a `Request`.
/// The response can be either `Ok` or `Err`, and may optionally include a message.
/// `Ok` will be interpreted as a successful execution (return 0),
/// while `Err` indicates a failure (return 1).
#[derive(Debug, PartialEq)]
pub enum Response {
    Ok(Option<String>),
    Err(Option<String>),
}

impl Response {
    /// Creates a new [`Response`] from a boolean value without a message.
    pub const fn from_bool(value: bool) -> Self {
        match value {
            true => Self::Ok(None),
            false => Self::Err(None),
        }
    }
}

impl ParentMessage for Response {
    fn into_bytes(self) -> Vec<u8> {
        let msg = match self {
            Self::Ok(Some(value)) => format!("OK {value}"),
            Self::Ok(None) => "OK".to_owned(),
            Self::Err(Some(value)) => format!("ERR {value}"),
            Self::Err(None) => "ERR".to_owned(),
        };
        msg.into()
    }
}

impl fmt::Display for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok(Some(value)) => write!(f, "OK {value}"),
            Self::Ok(None) => write!(f, "OK"),
            Self::Err(Some(value)) => write!(f, "ERR {value}"),
            Self::Err(None) => write!(f, "ERR"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_from_bytes_ok() {
        // raw data, (func type, args)
        let test_data = [
            (
                b"FN __resolve_eclass toolchain-funcs\n".to_vec(),
                (FuncType::ResolveEclass, vec!["toolchain-funcs"]),
            ),
            (
                b"FN debug-print $'llvm_gen_dep: entering function, parameters: \
                \\n\\t\\t\\tllvm-core/clang:${LLVM_SLOT}\\n\\t\\t\\t'\n"
                    .to_vec(),
                (
                    FuncType::DebugPrint,
                    vec![
                        "$llvm_gen_dep: entering function, parameters: \n\t\t\tllvm-core/clang:${LLVM_SLOT}\n\t\t\t",
                    ],
                ),
            ),
        ];

        for (data, (expected_func_type, expected_args)) in test_data {
            let req = Request::from_bytes(data).unwrap();
            assert_eq!(req.func, expected_func_type);
            assert_eq!(req.args, expected_args);
        }
    }
}
