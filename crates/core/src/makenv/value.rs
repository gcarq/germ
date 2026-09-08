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

    /// Expands and returns a string by substituting variables from the given `context`.
    /// The passed context must be in the original order.
    /// TODO: Add env.d to context for expansion.
    #[must_use = "this returns the expanded value as a new allocation"]
    pub fn expand(&self, context: &[(Box<str>, EnvValue)]) -> anyhow::Result<Self> {
        self.expand_with(|var| {
            context
                .iter()
                .rev()
                .find_map(|(ctx_var, ctx_value)| (var == ctx_var.as_ref()).then_some(ctx_value))
        })
    }

    /// Expands and returns a string by substituting variables from the given `lookup` function.
    pub fn expand_with<'ctx, F>(&self, lookup: F) -> anyhow::Result<Self>
    where
        F: Fn(&str) -> Option<&'ctx EnvValue>,
    {
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
            if let Some(ctx_value) = lookup(var) {
                new_value = new_value.replace(expr, &ctx_value.to_string());
            }
        }
        Ok(Self::new(new_value.as_str()))
    }

    /// Returns an iter over the inner values.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(AsRef::as_ref)
    }

    /// Normalize using incremental semantics.
    pub fn normalize(&mut self) {
        self.0 = Self::merge_values(self.iter());
    }

    /// Inherits the given `parent` with incremental semantics.
    pub fn inherit(&mut self, parent: &Self) {
        self.0 = Self::merge_values(parent.iter().chain(self.iter()));
    }

    /// Merges the given iterator of values using incremental semantics.
    ///
    /// Incremental semantics means that `-*` clears all previous values
    /// while `-foo` clears previous values that match `foo`,
    /// however `-foo` is not retained in the final result.
    fn merge_values<'a>(iter: impl Iterator<Item = &'a str>) -> Vec<Box<str>> {
        let mut values: Vec<Box<str>> = Vec::new();
        for value in iter {
            if value == "-*" {
                values.clear();
            } else if let Some(negated) = value.strip_prefix('-') {
                values.retain(|cur| cur.as_ref() != negated);
            } else if values.iter().find(|cur| cur.as_ref() == value).is_none() {
                values.push(value.into());
            }
        }
        values
    }
}

impl IntoIterator for EnvValue {
    type Item = Box<str>;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    /// Consumes self and returns an iter over the inner values.
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
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
            ("VAR2".into(), EnvValue::new("should not be used")),
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
