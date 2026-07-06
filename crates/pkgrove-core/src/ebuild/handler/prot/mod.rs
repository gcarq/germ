//! This module defines the protocol for the IPC between this and the ebuild process.
//!
//! The protocol is text-based and messages are terminated by `\4`.
//! There are two types of messages:
//!     * function call requests (prefixed with `FN`)
//!     * data messages (prefixed with `DATA`)
//!
//! Parameters for function calls are separated by `\0`, while data messages contain arbitrary text
//! and are not further parsed by the protocol.
//!
//! # Examples
//!
//! Function call request for `ver_test`:
//! `FN\0ver_test\06.0\0-gt\05.0\4`
//!
//! Data message from for `IUSE` variable:
//! `"DATA\0IUSE=static-libs tcpd usbip"`

pub mod func;

use crate::ebuild::handler::ipc::{ChildToParentMsg, ParentToChildMsg};
use crate::ebuild::handler::prot::func::FuncCall;
use anyhow::{Context, Result, anyhow};
use std::fmt;

/// Holds a message from the ebuild process, that can be either a function to execute [`FuncCall`]
/// or some `String` data.
pub enum ChildMessage {
    Call(FuncCall),
    Data(String),
}

impl ChildToParentMsg for ChildMessage {
    /// Creates a new [`ChildMessage`] from raw bytes received from the ebuild process
    /// excluding the end of text delimiter '\4'.
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        //trace!("Received message from child process: {bytes:?}");

        let msg = str::from_utf8(bytes).with_context(|| anyhow!("invalid UTF-8 in IPC message"))?;
        let mut parts = msg.split('\0');

        let msg = match parts.next() {
            Some("FN") => {
                let func = parts
                    .next()
                    .ok_or_else(|| anyhow!("missing function name in IPC msg '{msg}'"))?;
                let args = parts.collect::<Vec<&str>>();
                Self::Call(FuncCall::from_raw(func, &args)?)
            }
            Some("DATA") => Self::Data(parts.collect::<Vec<_>>().join(" ")),
            _ => Err(anyhow!("invalid IPC request: {msg}"))?,
        };
        Ok(msg)
    }
}

impl fmt::Display for ChildMessage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Call(func_call) => write!(f, "{func_call}"),
            Self::Data(data) => f.write_str(data),
        }
    }
}

/// Represents a response to be sent back to the ebuild process after handling a `ChildMessage`.
/// The response can be either `Ok` or `Err`, and may optionally include a message.
/// `Ok` will be interpreted as a successful execution (return 0),
/// while `Err` indicates a failure (return 1).
#[derive(PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub enum ParentMessage {
    Ok(Option<String>),
    Err(Option<String>),
}

impl ParentMessage {
    /// Creates a new [`ParentMessage`] from a boolean value without a `String`.
    pub const fn from_bool(value: bool) -> Self {
        match value {
            true => Self::Ok(None),
            false => Self::Err(None),
        }
    }
}

impl ParentToChildMsg for ParentMessage {
    fn into_bytes(self) -> Vec<u8> {
        self.to_string().into_bytes()
    }
}

impl fmt::Display for ParentMessage {
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
    fn test_child_message_from_bytes_ok() {
        // (raw data, expected message)
        let test_data = [
            (
                "FN\0ver_rs\01-\0' '",
                ChildMessage::Call(FuncCall {
                    func: func::FuncType::VerRs,
                    args: vec!["1-".to_owned(), "' '".to_owned()],
                }),
            ),
            (
                "FN\0ver_test\06.0\0-gt\05.0",
                ChildMessage::Call(FuncCall {
                    func: func::FuncType::VerTest,
                    args: vec!["6.0".to_owned(), "-gt".to_owned(), "5.0".to_owned()],
                }),
            ),
            (
                "DATA\0LICENSE=PSF-2",
                ChildMessage::Data("LICENSE=PSF-2".to_owned()),
            ),
            (
                "DATA\0IUSE=static-libs tcpd usbip",
                ChildMessage::Data("IUSE=static-libs tcpd usbip".to_owned()),
            ),
        ];

        for (data, expected_msg) in test_data {
            let msg = ChildMessage::from_bytes(data.as_bytes()).unwrap();
            assert_eq!(msg.to_string(), expected_msg.to_string());
        }
    }

    #[test]
    fn test_child_message_from_bytes_err() {
        let test_data = ["", "\0", "FN\0", "FOO\0bar\0baz"];

        for data in test_data {
            let msg = ChildMessage::from_bytes(data.as_bytes());
            assert!(msg.is_err(), "data '{data}' should be invalid");
        }
    }

    #[test]
    fn test_parent_message_into_bytes() {
        // (message, expected bytes)
        let test_data = [
            (ParentMessage::Ok(None), "OK".as_bytes()),
            (
                ParentMessage::Ok(Some("1.2.3".to_owned())),
                "OK 1.2.3".as_bytes(),
            ),
            (ParentMessage::Err(None), "ERR".as_bytes()),
            (
                ParentMessage::Err(Some("fatal error".to_owned())),
                "ERR fatal error".as_bytes(),
            ),
        ];

        for (msg, expected_bytes) in test_data {
            let bytes = msg.into_bytes();
            assert_eq!(bytes, expected_bytes);
        }
    }
}
