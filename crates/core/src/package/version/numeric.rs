use anyhow::bail;
use rkyv::{Archive, Deserialize, Serialize};
use std::cmp::Ordering;
use std::str::FromStr;
use std::{fmt, hash};

/// Represents a numeric value without a fixed width that uses numeric comparison.
#[derive(Archive, Serialize, Deserialize, Clone, Debug)]
pub struct NumericComponent(Box<str>);

impl NumericComponent {
    /// Creates a [`NumericComponent`] from the given `value`.
    ///
    /// Returns `Err` if the value is empty or contains invalid characters.
    pub fn new(value: &str) -> anyhow::Result<Self> {
        if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
            bail!("invalid numeric component: '{value}'");
        }
        Ok(Self::new_unchecked(value))
    }

    /// Creates a [`NumericComponent`] from the given `value` without validation.
    pub fn new_unchecked(value: &str) -> Self {
        Self(value.into())
    }

    /// Returns `true` if the numeric component is zero.
    pub fn is_zero(&self) -> bool {
        self.normalized().is_empty()
    }

    /// Returns the inner values without leading zeros for comparison purposes.
    pub fn normalized(&self) -> &str {
        self.0.trim_start_matches('0')
    }

    /// Returns the raw inner value.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for NumericComponent {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> anyhow::Result<Self> {
        Self::new(value)
    }
}

impl Ord for NumericComponent {
    fn cmp(&self, other: &Self) -> Ordering {
        let a = self.normalized();
        let b = other.normalized();
        a.len().cmp(&b.len()).then_with(|| a.cmp(b))
    }
}

impl PartialEq for NumericComponent {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for NumericComponent {}

impl PartialOrd for NumericComponent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for NumericComponent {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.normalized().hash(state);
    }
}

impl fmt::Display for NumericComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::assert_eq_hash;

    #[test]
    fn test_numeric_component_parse() {
        let component = NumericComponent::new("000123").unwrap();

        assert_eq!(component.as_str(), "000123");
        assert_eq!(component.to_string(), "000123");
        assert_eq!("456".parse::<NumericComponent>().unwrap().as_str(), "456");
    }

    #[test]
    fn test_numeric_component_parse_error() {
        for value in ["", "+1", "-1", " 1", "1 ", "１２"] {
            assert!(NumericComponent::new(value).is_err());
        }
    }

    #[test]
    fn test_numeric_component_equality() {
        for (left, right) in [("03", "3"), ("0", "000"), ("000123", "123")] {
            assert_eq_hash(
                &NumericComponent::new(left).unwrap(),
                &NumericComponent::new(right).unwrap(),
            );
        }
    }

    #[test]
    fn test_numeric_component_ordering() {
        let huge = NumericComponent::new("999999999999999999999999999999999").unwrap();
        let larger_huge = NumericComponent::new("1999999999999999999999999999999999").unwrap();

        assert!(huge < larger_huge);
    }
}
