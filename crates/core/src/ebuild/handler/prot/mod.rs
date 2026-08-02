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

use std::fmt;
use std::str::{FromStr, Utf8Error};
use thiserror::Error;

/// Field delimiter in messages from the ebuild process.
const FIELD_DELIMITER: char = '\0';

/// Frame delimiter for messages from the ebuild process.
pub(super) const CHILD_MESSAGE_DELIMITER: u8 = 0x04;

/// Frame delimiter for messages sent to the ebuild process.
pub(super) const PARENT_MESSAGE_DELIMITER: &[u8] = b"\n";

/// Errors caused by invalid ebuild IPC messages.
#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error("invalid UTF-8 in IPC request")]
    InvalidUtf8(#[from] Utf8Error),

    #[error("invalid IPC request: {0}")]
    InvalidRequest(String),

    #[error("missing function identifier in IPC request")]
    MissingFunction,

    #[error("unknown ebuild function '{func}'")]
    UnknownFunction { func: String },
}

/// All supported ebuild functions that can be called from the ebuild process.
#[derive(Debug, PartialEq)]
pub enum FuncType {
    ResolveEclass,
    DebugPrint,
    Die,
    Has,
    HasV,
    HasQ,
    VerCut,
    VerRs,
    VerTest,
}

impl FromStr for FuncType {
    type Err = ProtocolError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "__resolve_eclass" => Ok(Self::ResolveEclass),
            "debug-print" => Ok(Self::DebugPrint),
            "die" => Ok(Self::Die),
            "has" => Ok(Self::Has),
            "hasv" => Ok(Self::HasV),
            "hasq" => Ok(Self::HasQ),
            "ver_cut" => Ok(Self::VerCut),
            "ver_rs" => Ok(Self::VerRs),
            "ver_test" => Ok(Self::VerTest),
            _ => Err(ProtocolError::UnknownFunction {
                func: value.to_owned(),
            }),
        }
    }
}

impl fmt::Display for FuncType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::ResolveEclass => "__resolve_eclass",
            Self::DebugPrint => "debug-print",
            Self::Die => "die",
            Self::Has => "has",
            Self::HasV => "hasv",
            Self::HasQ => "hasq",
            Self::VerCut => "ver_cut",
            Self::VerRs => "ver_rs",
            Self::VerTest => "ver_test",
        };
        f.write_str(value)
    }
}

/// A recognized function call from the ebuild process.
#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct FuncCall {
    pub func: FuncType,
    pub args: Vec<String>,
}

impl FuncCall {
    pub(super) fn from_raw(func: &str, args: &[&str]) -> Result<Self, ProtocolError> {
        let func = FuncType::from_str(func)?;
        let args = args.iter().map(ToString::to_string).collect();
        Ok(Self { func, args })
    }
}

impl fmt::Display for FuncCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.func, self.args.join(" "))
    }
}

/// Holds a message from the ebuild process, that can be either a function to execute [`FuncCall`]
/// or some `String` data.
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum ChildMessage {
    Call(FuncCall),
    Data(String),
}

impl ChildMessage {
    /// Creates a new [`ChildMessage`] from raw bytes received from the ebuild process
    /// excluding the end of text delimiter [`CHILD_MESSAGE_DELIMITER`].
    ///
    /// # Errors
    ///
    /// Returns an error if the message cannot be parsed, or specifies an invalid function.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let msg = str::from_utf8(bytes)?;
        let mut parts = msg.split(FIELD_DELIMITER);

        let request = match parts.next() {
            Some("FN") => {
                let func = parts
                    .next()
                    .filter(|func| !func.is_empty())
                    .ok_or(ProtocolError::MissingFunction)?;
                let args = parts.collect::<Vec<&str>>();
                Self::Call(FuncCall::from_raw(func, &args)?)
            }
            Some("DATA") => Self::Data(parts.collect::<Vec<_>>().join(" ")),
            _ => return Err(ProtocolError::InvalidRequest(msg.to_owned())),
        };
        Ok(request)
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
/// The response can be `Ok`, `Err`, or `Die`. `Ok` and `Err` may optionally include a message.
/// `Ok` will be interpreted as a successful execution (return 0), `Err` as a function failure
/// (return 1), and `Die` terminates the ebuild process with status 1.
#[derive(PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub enum ParentMessage {
    Ok(Option<String>),
    Err(Option<String>),
    Die(String),
}

impl ParentMessage {
    /// Creates a new [`ParentMessage`] from a boolean value without a `String`.
    pub const fn from_bool(value: bool) -> Self {
        match value {
            true => Self::Ok(None),
            false => Self::Err(None),
        }
    }

    /// Encodes the response without its transport framing delimiter.
    pub fn into_bytes(self) -> Vec<u8> {
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
            Self::Die(_) => write!(f, "DIE"),
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
                "FN\x00ver_rs\x001-\x00' '",
                ChildMessage::Call(FuncCall {
                    func: FuncType::VerRs,
                    args: vec!["1-".to_owned(), "' '".to_owned()],
                }),
            ),
            (
                "FN\x00ver_test\x006.0\x00-gt\x005.0",
                ChildMessage::Call(FuncCall {
                    func: FuncType::VerTest,
                    args: vec!["6.0".to_owned(), "-gt".to_owned(), "5.0".to_owned()],
                }),
            ),
            (
                "DATA\x00LICENSE=PSF-2",
                ChildMessage::Data("LICENSE=PSF-2".to_owned()),
            ),
            (
                "DATA\x00IUSE=static-libs tcpd usbip",
                ChildMessage::Data("IUSE=static-libs tcpd usbip".to_owned()),
            ),
        ];

        for (data, expected_msg) in test_data {
            let msg = ChildMessage::from_bytes(data.as_bytes()).unwrap();
            assert_eq!(msg, expected_msg);
        }
    }

    #[test]
    fn test_child_message_from_bytes_invalid_request() {
        assert!(matches!(
            ChildMessage::from_bytes(b"FOO\0bar\0baz"),
            Err(ProtocolError::InvalidRequest(request)) if request == "FOO\0bar\0baz"
        ));
    }

    #[test]
    fn test_child_message_from_bytes_invalid_utf8() {
        assert!(matches!(
            ChildMessage::from_bytes(&[0xff]),
            Err(ProtocolError::InvalidUtf8(_))
        ));
    }

    #[test]
    fn test_child_message_from_bytes_missing_function() {
        assert!(matches!(
            ChildMessage::from_bytes(b"FN\0"),
            Err(ProtocolError::MissingFunction)
        ));
    }

    #[test]
    fn test_child_message_from_bytes_unknown_function() {
        assert!(matches!(
            ChildMessage::from_bytes(b"FN\0unknown"),
            Err(ProtocolError::UnknownFunction { func }) if func == "unknown"
        ));
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
            (ParentMessage::Die("fatal error".into()), "DIE".as_bytes()),
        ];

        for (msg, expected_bytes) in test_data {
            let bytes = msg.into_bytes();
            assert_eq!(bytes, expected_bytes);
        }
    }
}
