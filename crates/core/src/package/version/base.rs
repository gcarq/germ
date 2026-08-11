use super::numeric::NumericComponent;
use anyhow::{anyhow, bail};
use rkyv::{Archive, Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt::Write;
use std::str::FromStr;
use std::{fmt, hash};

/// Represents the base version number as individual components and an optional letter suffix.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct VersionNumber {
    components: Box<[NumberComponent]>,
    letter: Option<char>,
}

impl VersionNumber {
    /// Creates a [`VersionNumber`] by splitting the version into its numeric components
    /// and an optional letter suffix.
    ///
    /// For example, `"1.2.3a"` becomes `(["1", "2", "3", "a"], Some('a'))` while
    /// `"2.0.1"` becomes `(["2", "0", "1"], None)`.
    fn new(version: &str) -> anyhow::Result<Self> {
        let (version, letter) = match version
            .chars()
            .last()
            .ok_or_else(|| anyhow!("unable to parse version number from: '{version}'"))?
        {
            c @ 'a'..='z' => (&version[..version.len() - 1], Some(c)),
            c if !c.is_ascii_digit() => {
                bail!("invalid version suffix character in '{version}'");
            }
            _ => (version, None),
        };

        let components = version
            .split('.')
            .enumerate()
            .map(|(idx, comp)| NumberComponent::new(comp, idx))
            .collect::<anyhow::Result<_>>()?;
        Ok(Self { components, letter })
    }

    /// Returns an iterator over the components.
    pub fn components(&self) -> impl Iterator<Item = &NumberComponent> {
        self.components.iter()
    }

    pub const fn letter(&self) -> Option<char> {
        self.letter
    }
}

impl FromStr for VersionNumber {
    type Err = anyhow::Error;

    fn from_str(version: &str) -> anyhow::Result<Self> {
        Self::new(version)
    }
}

impl Ord for VersionNumber {
    fn cmp(&self, other: &Self) -> Ordering {
        match self
            .components
            .iter()
            .zip(other.components.iter())
            .map(|(a, b)| a.cmp(b))
            .find(|o| *o != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
        {
            Ordering::Equal => {}
            non_equal => return non_equal,
        }

        match self.components.len().cmp(&other.components.len()) {
            Ordering::Equal => {}
            non_equal => return non_equal,
        }

        match (self.letter, other.letter) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            (None, None) => Ordering::Equal,
        }
    }
}

impl PartialEq<Self> for VersionNumber {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl PartialOrd<Self> for VersionNumber {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for VersionNumber {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.components.hash(state);
        self.letter.hash(state);
    }
}

impl fmt::Display for VersionNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut iter = self.components.iter();
        if let Some(first) = iter.next() {
            write!(f, "{first}")?;
        }
        for comp in iter {
            f.write_char('.')?;
            write!(f, "{comp}")?;
        }
        if let Some(letter) = self.letter {
            f.write_char(letter)?;
        }
        Ok(())
    }
}

/// Represents a component of the base version, either `Numeric` or `Alphabetic`.
/// The distinction is important for comparison purposes, components starting with a '0' are
/// considered Alphabetic and are compared as strings, while Numeric and compared as integers.
/// Nevertheless, both variants should only contain digits.
/// E.g., in "1.2.03a", "1" and "2" are Numeric, "03" is Alphabetic, and "a" is handled separately
/// and not part of this enum.
/// See PMS 3.2 and 3.3 for more details.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, Debug)]
pub enum NumberComponent {
    Numeric(NumericComponent),
    Alphabetic(Box<str>),
}

impl NumberComponent {
    /// Creates a new [`NumberComponent`] from the given `number` and its index in the version.
    /// The first component (index 0) is always considered `Numeric`.
    /// Subsequent components starting with '0' are considered `Alphabetic`.
    /// Returns an `Err` if the `number` is empty or contains non-digit characters.
    pub fn new(number: &str, index: usize) -> anyhow::Result<Self> {
        if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
            bail!("invalid version component: '{number}'");
        }
        let component = match index == 0 || !number.starts_with('0') {
            true => NumberComponent::Numeric(NumericComponent::new_unchecked(number)),
            false => NumberComponent::Alphabetic(number.into()),
        };
        Ok(component)
    }
}

impl Ord for NumberComponent {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (NumberComponent::Numeric(a), NumberComponent::Numeric(b)) => a.cmp(b),
            (NumberComponent::Alphabetic(a), NumberComponent::Alphabetic(b)) => {
                a.trim_end_matches('0').cmp(b.trim_end_matches('0'))
            }
            (NumberComponent::Numeric(a), NumberComponent::Alphabetic(b)) => a
                .as_str()
                .trim_end_matches('0')
                .cmp(b.trim_end_matches('0')),
            (NumberComponent::Alphabetic(a), NumberComponent::Numeric(b)) => a
                .trim_end_matches('0')
                .cmp(b.as_str().trim_end_matches('0')),
        }
    }
}

impl PartialEq<Self> for NumberComponent {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl PartialOrd<Self> for NumberComponent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for NumberComponent {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        match self {
            NumberComponent::Numeric(n) => n.hash(state),
            NumberComponent::Alphabetic(a) => a.trim_end_matches('0').hash(state),
        }
    }
}

impl fmt::Display for NumberComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NumberComponent::Numeric(n) => f.write_str(n.as_str()),
            NumberComponent::Alphabetic(a) => f.write_str(a),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::assert_eq_hash;

    #[test]
    fn test_version_number_parse() {
        assert_eq_hash(
            &VersionNumber::from_str("1.2.3a").unwrap(),
            &VersionNumber {
                components: [
                    NumberComponent::Numeric("1".parse().unwrap()),
                    NumberComponent::Numeric("2".parse().unwrap()),
                    NumberComponent::Numeric("3".parse().unwrap()),
                ]
                .into(),
                letter: Some('a'),
            },
        );

        assert_eq_hash(
            &VersionNumber::from_str("2.0.1").unwrap(),
            &VersionNumber {
                components: [
                    NumberComponent::Numeric("2".parse().unwrap()),
                    NumberComponent::Numeric("0".parse().unwrap()),
                    NumberComponent::Numeric("1".parse().unwrap()),
                ]
                .into(),
                letter: None,
            },
        );

        assert_eq_hash(
            &VersionNumber::from_str("1.2.03").unwrap(),
            &VersionNumber {
                components: [
                    NumberComponent::Numeric("1".parse().unwrap()),
                    NumberComponent::Numeric("2".parse().unwrap()),
                    NumberComponent::Alphabetic("03".into()),
                ]
                .into(),
                letter: None,
            },
        );

        assert_eq_hash(
            &VersionNumber::from_str("20251122").unwrap(),
            &VersionNumber {
                components: [NumberComponent::Numeric("20251122".parse().unwrap())].into(),
                letter: None,
            },
        );
        assert_eq_hash(
            &VersionNumber::from_str("1.030").unwrap(),
            &VersionNumber::from_str("1.03").unwrap(),
        );
    }

    #[test]
    fn test_version_number_parse_error() {
        let invalid_versions = ["", ".", "1..2", "1.2.3A", "1.2.3!"];
        for version in invalid_versions {
            assert!(
                VersionNumber::from_str(version).is_err(),
                "Version '{version}' should be invalid"
            );
        }
    }

    #[test]
    fn test_version_number_display() {
        let test_cases: [(VersionNumber, &str); 4] = [
            ("1.2.3a".parse().unwrap(), "1.2.3a"),
            ("2.0.1".parse().unwrap(), "2.0.1"),
            ("1.2.03".parse().unwrap(), "1.2.03"),
            ("20251122".parse().unwrap(), "20251122"),
        ];
        for (version, expected) in test_cases {
            assert_eq!(version.to_string(), expected);
        }
    }

    #[test]
    fn test_number_component_ordering() {
        let alpha03 = NumberComponent::Alphabetic("03".into());
        let alpha3 = NumberComponent::Alphabetic("3".into());

        assert!(alpha03 < alpha3); // Alphabetic components compare as ASCII
    }

    #[test]
    fn test_number_component_display() {
        let test_cases = [
            (NumberComponent::Numeric("2".parse().unwrap()), "2"),
            (NumberComponent::Alphabetic("03".into()), "03"),
            (NumberComponent::Numeric("3".parse().unwrap()), "3"),
        ];
        for (component, expected) in test_cases {
            assert_eq!(component.to_string(), expected);
        }
    }
}
