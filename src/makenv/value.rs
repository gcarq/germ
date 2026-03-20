use crate::utils::Inherit;
use regex::Regex;
use std::fmt;
use std::sync::LazyLock;

/// Regex to capture variable references for expansion.
static VAR_EXPAND_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?<expr>\$\{?(?<var>[a-zA-Z][a-zA-Z0-9_]*)\}?)").unwrap());

/// Represents a variable value in portage configuration files.
/// It supports simple shell-like expansion in the form "${VAR}" or "$VAR".
/// A value can be either a literal or incremental value.
/// See `man make.conf` for more information.
#[derive(Clone)]
pub enum EnvValue {
    Literal(Vec<String>),
    Incremental(Vec<String>),
}

impl EnvValue {
    pub fn new(value: String, is_incremental: bool) -> Self {
        let values = value
            .split_ascii_whitespace()
            .map(ToOwned::to_owned)
            .collect();
        if is_incremental {
            EnvValue::Incremental(values)
        } else {
            EnvValue::Literal(values)
        }
    }

    pub const fn is_incremental(&self) -> bool {
        matches!(self, EnvValue::Incremental(_))
    }

    /// Expands and returns a string value by substituting variables from the given context.
    /// The passed context must be in the original order.
    /// TODO: Add env.d to context for expansion.
    #[must_use = "this returns the expanded value as a new allocation"]
    pub fn expand(&self, context: &[(String, EnvValue)]) -> Self {
        let value = match self {
            EnvValue::Literal(values) | EnvValue::Incremental(values) => values,
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

    pub fn inner(&self) -> &[String] {
        match self {
            EnvValue::Literal(values) | EnvValue::Incremental(values) => values,
        }
    }

    /// Consumes self and returns the inner value.
    pub fn into_inner(self) -> Vec<String> {
        match self {
            EnvValue::Literal(values) | EnvValue::Incremental(values) => values,
        }
    }
}

impl Inherit for EnvValue {
    /// Inherits the value of the given `parent`.
    ///
    /// Only Incremental values can be inherited, otherwise this method does nothing.
    /// See PMS 5.3.1 for the inheritance rules.
    fn inherit_from(&mut self, parent: &Self) {
        let (EnvValue::Incremental(values), EnvValue::Incremental(parent)) = (&self, parent) else {
            return;
        };
        let mut new_values = Vec::new();
        for value in parent.iter().chain(values) {
            if value == "-*" {
                new_values.clear();
            } else if let Some(negated) = value.strip_prefix('-') {
                new_values.retain(|v| v != negated);
            } else if !new_values.contains(value) {
                new_values.push(value.clone());
            }
        }
        //values.sort_unstable();
        *self = EnvValue::Incremental(new_values);
    }
}

impl fmt::Display for EnvValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            EnvValue::Literal(values) | EnvValue::Incremental(values) => values,
        }
        .join(" ");
        f.write_str(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_env_value_inherit_from_incremental() {
        let parent = EnvValue::new("X branding -* asm accessibility".into(), true);
        let mut child = EnvValue::new("blas -accessibility".into(), true);
        child.inherit_from(&parent);
        assert_eq!(child.to_string(), "asm blas");
    }

    #[test]
    fn test_env_value_inherit_from_literal() {
        let parent = EnvValue::new("X branding -*".into(), false);
        let mut child = EnvValue::new("blas".into(), false);
        child.inherit_from(&parent);
        assert_eq!(child.to_string(), "blas");
    }

    #[test]
    fn test_env_value_to_string() {
        let literal = EnvValue::new("value1 value2".into(), false);
        assert_eq!(literal.to_string(), "value1 value2");

        let incremental = EnvValue::new("value1 value2".into(), true);
        assert_eq!(incremental.to_string(), "value1 value2");
    }
}
