//! This module defines the protocol for the IPC between this and the ebuild process.
use anyhow::{Result, anyhow};
use std::fmt;
use std::str::FromStr;
use strum::{Display, EnumString};

/// All supported ebuild functions that can be called from the ebuild process.
#[derive(EnumString, Display)]
pub enum FuncType {
    // Internal ebuild functions
    #[strum(serialize = "__resolve_eclass")]
    ResolveEclass,

    // Misc Commands
    #[strum(serialize = "contains_word")]
    ContainsWord,
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
pub struct Request<'a> {
    pub func: FuncType,
    pub args: &'a [&'a str],
}

impl<'a> Request<'a> {
    pub fn new(data: &'a [&'a str]) -> Result<Self> {
        match data {
            ["FN", func_name, args @ ..] => {
                let func = FuncType::from_str(func_name)?;
                Ok(Self { func, args })
            }
            _ => Err(anyhow!("invalid ebuild IPC request: {data:?}")),
        }
    }
}

impl fmt::Display for Request<'_> {
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
    pub fn from_bool(value: bool) -> Self {
        match value {
            true => Self::Ok(None),
            false => Self::Err(None),
        }
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
