pub mod repos;

use crate::conf::repos::ReposConf;
use crate::makenv::MakeEnv;
use crate::profile::{InheritFrom, Profile};
use crate::utils::FileFromPath;
use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use std::fmt;
use std::path::Path;

lazy_static! {
    /// Regex to capture variable references for expansion.
    static ref VAR_EXPAND_RE: Regex = Regex::new(r"(?<expr>\$\{?(?<var>[a-zA-Z][a-zA-Z0-9_]*)\}?)").unwrap();
}

/// Holds the portage configuration that usually resides in /etc/portage.
#[derive(Debug)]
pub struct PortageConf {
    pub make_env: MakeEnv,
    pub repos: ReposConf,
    profile: Profile,
}

impl PortageConf {
    /// Builds a [`PortageConf`] from the given portage configuration path.
    pub fn new(path: &Path) -> Result<Self> {
        let repos = ReposConf::new(&path.join("repos.conf"))
            .with_context(|| "Unable to process repos.conf")?;
        let mut make_env = MakeEnv::from_path(&path.join("make.conf"), true, false)
            .with_context(|| "Unable to process make.conf")?;
        let profile = Profile::new(&path.join("make.profile"), &repos)
            .with_context(|| "Unable to build profile from make.profile")?;

        make_env.inherit_from(&profile.make_defaults);

        Ok(PortageConf {
            make_env,
            profile,
            repos,
        })
    }
}

/// Represents a variable value in portage configuration files.
/// It supports simple shell-like expansion in the form "${VAR}" or "$VAR".
/// A value can be either a literal or incremental value.
/// See `man make.conf` for more information.
#[derive(Debug, Clone)]
pub enum EnvValue {
    Literal(String),
    Incremental(Vec<String>),
}

impl EnvValue {
    pub fn new(value: String, is_incremental: bool) -> Self {
        if is_incremental {
            let values = value
                .split_ascii_whitespace()
                .map(|s| s.to_string())
                .collect::<Vec<String>>();
            EnvValue::Incremental(values)
        } else {
            EnvValue::Literal(value)
        }
    }

    pub fn value(&self) -> String {
        match self {
            EnvValue::Literal(value) => value.clone(),
            EnvValue::Incremental(values) => values.join(" "),
        }
    }

    /// Expands and returns a string value by substituting variables from the given context.
    /// The passed context must be in the original order.
    #[must_use = "this returns the expanded value as a new allocation"]
    pub fn expand(&self, context: &[(String, EnvValue)]) -> EnvValue {
        let (value, is_incremental) = match self {
            EnvValue::Literal(value) => (value.clone(), false),
            EnvValue::Incremental(values) => (values.join(" "), true),
        };
        let mut new_value = value.clone();

        for cap in VAR_EXPAND_RE.captures_iter(&value) {
            for (ctx_var, ctx_value) in context.iter().rev() {
                if cap["var"] == *ctx_var {
                    new_value = new_value.replace(&cap["expr"], &ctx_value.to_string());
                    break;
                }
            }
        }
        EnvValue::new(new_value, is_incremental)
    }
}

impl fmt::Display for EnvValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            EnvValue::Literal(value) => value.clone(),
            EnvValue::Incremental(values) => values.join(" "),
        };
        write!(f, "{value}")
    }
}
