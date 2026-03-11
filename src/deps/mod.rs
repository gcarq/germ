pub mod atom;
pub mod expr;
mod parser;

use anyhow::{Result, anyhow};
use expr::ExpressionItem;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

/// Represents a USE flag
#[derive(Serialize, Deserialize, Clone, Eq, PartialEq)]
#[cfg_attr(test, derive(Default, Debug))]
pub struct UseFlag(Box<str>);

impl ExpressionItem for UseFlag {}

impl FromStr for UseFlag {
    type Err = anyhow::Error;

    // TODO: implement name validation
    fn from_str(s: &str) -> Result<Self> {
        if s.is_empty() {
            return Err(anyhow!("use flag cannot be empty"));
        }
        Ok(UseFlag(s.into()))
    }
}

impl fmt::Display for UseFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Deref for UseFlag {
    type Target = Box<str>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
