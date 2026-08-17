use anyhow::anyhow;
use fancy_regex::Regex;
use std::fmt;
use std::sync::LazyLock;

/// Regex to capture variable references for expansion.
static VAR_EXPAND_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?<expr>\$\{?(?<var>[a-zA-Z][a-zA-Z0-9_]*)\}?)").unwrap());

/// Represents a variable value in portage configuration files.
/// It supports simple shell-like expansion in the form "${VAR}" or "$VAR".
#[derive(Clone, Default)]
pub struct EnvValue(Vec<Box<str>>);

impl EnvValue {
    pub fn new<'a>(value: impl Into<&'a str>) -> Self {
        Self(
            value
                .into()
                .split_ascii_whitespace()
                .map(Into::into)
                .collect(),
        )
    }

    /// Expands and returns a string value by substituting variables from the given context.
    /// The passed context must be in the original order.
    /// TODO: Add env.d to context for expansion.
    #[must_use = "this returns the expanded value as a new allocation"]
    pub fn expand(&self, context: &[(Box<str>, EnvValue)]) -> anyhow::Result<Self> {
        if !self.0.iter().any(|value| value.contains('$')) {
            return Ok(self.clone());
        }

        let value = self.0.join(" ");
        let mut new_value = value.clone();

        for cap in VAR_EXPAND_RE.captures_iter(&value) {
            let cap = cap?;
            let var = cap
                .name("var")
                .ok_or_else(|| anyhow!("variable expansion is missing a variable"))?
                .as_str();
            let expr = cap
                .name("expr")
                .ok_or_else(|| anyhow!("variable expansion is missing an expression"))?
                .as_str();
            for (ctx_var, ctx_value) in context.iter().rev() {
                if var == ctx_var.as_ref() {
                    new_value = new_value.replace(expr, &ctx_value.to_string());
                    break;
                }
            }
        }
        Ok(Self::new(new_value.as_str()))
    }

    pub fn inner(&self) -> &[Box<str>] {
        &self.0
    }

    /// Consumes self and returns the inner value.
    pub fn into_inner(self) -> Vec<Box<str>> {
        self.0
    }

    fn merge_values<'a>(iter: impl Iterator<Item = &'a Box<str>>) -> Vec<Box<str>> {
        let mut values: Vec<Box<str>> = Vec::new();
        for value in iter {
            if value.as_ref() == "-*" {
                values.clear();
            } else if let Some(negated) = value.strip_prefix('-') {
                values.retain(|cur| cur.as_ref() != negated);
            } else if !values.contains(value) {
                values.push(value.clone());
            }
        }
        values
    }

    /// Normalize using incremental semantics.
    pub fn normalize(&mut self) {
        self.0 = Self::merge_values(self.inner().iter());
    }

    /// Inherits the given `parent` with incremental semantics.
    pub fn inherit(&mut self, parent: &Self) {
        self.0 = Self::merge_values(parent.inner().iter().chain(self.inner()));
    }
}

impl fmt::Display for EnvValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_value_expand() {
        let context = vec![
            ("VAR1".into(), EnvValue::new("value1")),
            ("VAR2".into(), EnvValue::new("value2")),
        ];
        let value = EnvValue::new("${VAR1} $VAR2 ${VAR3}");
        assert_eq!(
            value.expand(&context).unwrap().to_string(),
            "value1 value2 ${VAR3}"
        );
    }

    #[test]
    fn test_env_value_merge_incremental() {
        let parent = EnvValue::new("X branding -* asm accessibility");
        let mut child = EnvValue::new("blas -accessibility");
        child.inherit(&parent);
        assert_eq!(child.to_string(), "asm blas");
    }

    #[test]
    fn test_env_value_display() {
        let value = EnvValue::new("value1 value2");
        assert_eq!(value.to_string(), "value1 value2");
    }
}
