use crate::profile::InheritFrom;
use crate::utils;
use crate::utils::FileFromPath;
use anyhow::{Context, Result, anyhow};
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashMap;
use std::fmt;

lazy_static! {
    /// Regex to capture variable references for expansion.
    static ref VAR_EXPAND_RE: Regex = Regex::new(r"(?<expr>\$\{?(?<var>[a-zA-Z][a-zA-Z0-9_]*)\}?)").unwrap();
}

/// List of variables that are incremental as per PMS section 5.3 and
/// https://github.com/gentoo/portage/blob/0783d820e6eecffa3adff52c4669fc715d65dbaa/lib/portage/const.py#L121
/// NOTE: This list must be kept sorted for [`core::slice::binary_search`] to work correctly.
const INCREMENTAL_VARIABLES: [&str; 14] = [
    "ACCEPT_KEYWORDS",
    "CONFIG_PROTECT",
    "CONFIG_PROTECT_MASK",
    "ENV_UNSET",
    "FEATURES",
    "IUSE_IMPLICIT",
    "PRELINK_PATH",
    "PRELINK_PATH_MASK",
    "PROFILE_ONLY_VARIABLES",
    "USE",
    "USE_EXPAND",
    "USE_EXPAND_HIDDEN",
    "USE_EXPAND_IMPLICIT",
    "USE_EXPAND_UNPREFIXED",
];

/// Holds all environment variables defined in a make.conf or make.defaults file.
#[derive(Debug, Default)]
pub struct MakeEnv {
    vars: HashMap<String, EnvValue>,
}

impl MakeEnv {
    pub fn get(&self, key: &str) -> Option<&EnvValue> {
        self.vars.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &EnvValue)> {
        self.vars.iter()
    }
}

impl FileFromPath for MakeEnv {
    /// Builds a [`MakeEnv`] from the given content of a make.conf or make.defaults file.
    fn from_file_content(content: String) -> Result<Self>
    where
        Self: Sized,
    {
        let mut vars = utils::shlex_split(content)?
            .into_iter()
            .map(|(key, value)| {
                if key
                    .chars()
                    .next()
                    .with_context(|| "variable name cannot be empty")?
                    .is_ascii_alphabetic()
                {
                    let is_incremental = INCREMENTAL_VARIABLES.binary_search(&key.as_str()).is_ok();
                    Ok((key, EnvValue::new(value, is_incremental)))
                } else {
                    Err(anyhow!("invalid variable name: {key}"))
                }
            })
            .collect::<Result<Vec<(String, EnvValue)>>>()?;

        for i in 0..vars.len() {
            vars[i].1 = vars[i].1.expand(&vars[..i]);
        }

        let vars = vars.into_iter().collect::<HashMap<String, EnvValue>>();
        Ok(Self { vars })
    }
}

impl InheritFrom for MakeEnv {
    fn inherit_from(&mut self, parent: &MakeEnv) {
        let parent_ctx = Vec::from_iter(parent.vars.iter().map(|(k, v)| (k.clone(), v.clone())));
        for (key, parent_value) in &parent.vars {
            // If the variable exists, expand it with the parent's context and take care of
            // incremental variables, otherwise just insert it.
            match self.vars.get_mut(key) {
                Some(self_value) => {
                    *self_value = self_value.expand(parent_ctx.as_slice());
                    if let EnvValue::Incremental(self_v) = self_value
                        && let EnvValue::Incremental(parent_v) = parent_value
                    {
                        let mut values = Vec::new();
                        for value in parent_v.iter().chain(self_v.iter()) {
                            if value == "-*" {
                                values.clear();
                            } else if value.starts_with('-') {
                                values.retain(|v| v != &value[1..]);
                            } else if !values.contains(value) {
                                values.push(value.clone());
                            }
                        }
                        //values.sort_unstable();
                        *self_value = EnvValue::Incremental(values);
                    }
                }
                None => {
                    self.vars.insert(key.clone(), parent_value.clone());
                }
            }
        }
    }
}

/// Represents a variable value in portage configuration files.
/// It supports simple shell-like expansion in the form "${VAR}" or "$VAR".
/// A value can be either a literal or incremental value.
/// See `man make.conf` for more information.
#[derive(Debug, Clone)]
pub enum EnvValue {
    Literal(Vec<String>),
    Incremental(Vec<String>),
}

impl EnvValue {
    pub fn new(value: String, is_incremental: bool) -> Self {
        let values = value
            .split_ascii_whitespace()
            .map(|s| s.to_string())
            .collect();
        if is_incremental {
            EnvValue::Incremental(values)
        } else {
            EnvValue::Literal(values)
        }
    }

    pub fn is_incremental(&self) -> bool {
        matches!(self, EnvValue::Incremental(_))
    }

    /// Expands and returns a string value by substituting variables from the given context.
    /// The passed context must be in the original order.
    /// TODO: Add env.d to context for expansion.
    #[must_use = "this returns the expanded value as a new allocation"]
    pub fn expand(&self, context: &[(String, EnvValue)]) -> EnvValue {
        let value = match self {
            EnvValue::Literal(values) => values,
            EnvValue::Incremental(values) => values,
        }
        .join(" ");
        let mut new_value = value.clone();

        for cap in VAR_EXPAND_RE.captures_iter(&value) {
            for (ctx_var, ctx_value) in context.iter().rev() {
                if cap["var"] == *ctx_var {
                    new_value = new_value.replace(&cap["expr"], &ctx_value.to_string());
                    break;
                }
            }
        }
        EnvValue::new(new_value, self.is_incremental())
    }
}

impl fmt::Display for EnvValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            EnvValue::Literal(values) => values,
            EnvValue::Incremental(values) => values,
        }
        .join(" ");
        write!(f, "{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_env_from_file_content() {
        let content = r#"
# This is a comment
USE="cet"

# Should result in BOOTSTRAP_USE="${BOOTSTRAP_USE} cet"
BOOTSTRAP_USE="${BOOTSTRAP_USE}
$USE"

# Should result in USE="cet -foo -bar"
USE="${USE} -foo"
USE="${USE} -bar"

enable_year2038="no"
        "#;
        let make_env = MakeEnv::from_file_content(content.to_string()).unwrap();
        assert_eq!(make_env.get("USE").unwrap().to_string(), "cet -foo -bar");
        assert_eq!(
            make_env.get("BOOTSTRAP_USE").unwrap().to_string(),
            "${BOOTSTRAP_USE} cet"
        );
        assert_eq!(make_env.get("enable_year2038").unwrap().to_string(), "no");
    }

    #[test]
    fn test_make_env_inherit_from() {
        let parent_content = r#"
USE="cet -iconv"
        "#;
        let child_content = r#"
USE="${USE} seccomp branding -cet"
        "#;
        let parent = MakeEnv::from_file_content(parent_content.into()).unwrap();
        let mut child = MakeEnv::from_file_content(child_content.into()).unwrap();
        child.inherit_from(&parent);
        assert_eq!(child.get("USE").unwrap().to_string(), "seccomp branding");
    }

    #[test]
    fn test_env_value_expand() {
        let context = vec![
            ("VAR1".into(), EnvValue::new("value1".into(), false)),
            ("VAR2".into(), EnvValue::new("value2".into(), true)),
        ];
        let value = EnvValue::new("${VAR1} $VAR2 ${VAR3}".into(), false);
        assert_eq!(value.expand(&context).to_string(), "value1 value2 ${VAR3}");
    }

    #[test]
    fn test_env_value_to_string() {
        let literal = EnvValue::new("value1 value2".into(), false);
        assert_eq!(literal.to_string(), "value1 value2");

        let incremental = EnvValue::new("value1 value2".into(), true);
        assert_eq!(incremental.to_string(), "value1 value2");
    }
}
