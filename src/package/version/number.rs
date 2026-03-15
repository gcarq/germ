use anyhow::{Result, anyhow};
use rkyv::{Archive, Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt::Write;
use std::str::FromStr;
use std::{fmt, hash};

/// Represents the base version number as individual components and an optional letter suffix.
#[derive(Archive, Serialize, Deserialize, Clone, Eq)]
#[cfg_attr(test, derive(Default, Debug))]
pub struct VersionNumber {
    components: Box<[NumberComponent]>,
    letter: Option<char>,
}

impl VersionNumber {
    /// Returns an iterator over the string representations of each component
    /// including the optional letter suffix as the last element.
    pub fn iter(&self) -> impl Iterator<Item = String> {
        self.components
            .iter()
            .map(ToString::to_string)
            .chain(self.letter.map(|c| c.to_string()))
    }
}

impl FromStr for VersionNumber {
    type Err = anyhow::Error;

    /// Creates a [`VersionNumber`] by splitting the version into its numeric components
    /// and an optional letter suffix.
    ///
    /// For example, `"1.2.3a"` becomes `(["1", "2", "3", "a"], Some('a'))` while
    /// `"2.0.1"` becomes `(["2", "0", "1"], None)`.
    fn from_str(version: &str) -> Result<Self> {
        let (version, letter) = match version
            .chars()
            .last()
            .ok_or_else(|| anyhow!("unable to parse version number from: '{version}'"))?
        {
            c @ 'a'..='z' => (&version[..version.len() - 1], Some(c)),
            c if !c.is_ascii_digit() => {
                return Err(anyhow!("invalid version suffix character in '{version}'"));
            }
            _ => (version, None),
        };

        let components = version
            .split('.')
            .enumerate()
            .map(|(idx, comp)| NumberComponent::new(comp, idx))
            .collect::<Result<_>>()?;
        Ok(Self { components, letter })
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
#[derive(Archive, Serialize, Deserialize, Clone, Eq)]
#[cfg_attr(test, derive(Debug))]
enum NumberComponent {
    Numeric(Box<str>),
    Alphabetic(Box<str>),
}

impl NumberComponent {
    /// Creates a new [`NumberComponent`] from the given `number` and its index in the version.
    /// The first component (index 0) is always considered `Numeric`.
    /// Subsequent components starting with '0' are considered `Alphabetic`.
    /// Returns an `Err` if the `number` is empty or contains non-digit characters.
    pub fn new(number: &str, index: usize) -> Result<Self> {
        if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
            return Err(anyhow!("invalid version component: '{number}'"));
        }
        let component = match index == 0 || !number.starts_with('0') {
            true => NumberComponent::Numeric(number.into()),
            false => NumberComponent::Alphabetic(number.into()),
        };
        Ok(component)
    }
}

impl Ord for NumberComponent {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            // Compare as integers, safe to unwrap as it can only contain numbers at this point.
            (NumberComponent::Numeric(a), NumberComponent::Numeric(b)) => a
                .parse::<usize>()
                .unwrap()
                .cmp(&b.parse::<usize>().unwrap()),
            (NumberComponent::Alphabetic(a), NumberComponent::Alphabetic(b))
            | (NumberComponent::Numeric(a), NumberComponent::Alphabetic(b))
            | (NumberComponent::Alphabetic(a), NumberComponent::Numeric(b)) => {
                a.trim_end_matches('0').cmp(b.trim_end_matches('0'))
            }
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
        let comp = match self {
            NumberComponent::Numeric(n) => n,
            NumberComponent::Alphabetic(a) => a.trim_start_matches('0'),
        };
        comp.hash(state);
    }
}

impl fmt::Display for NumberComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let repr = match self {
            NumberComponent::Numeric(n) => n,
            NumberComponent::Alphabetic(a) => a,
        };
        f.write_str(repr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_number_new_ok() {
        assert_eq!(
            VersionNumber::from_str("1.2.3a").unwrap(),
            VersionNumber {
                components: [
                    NumberComponent::Numeric("1".into()),
                    NumberComponent::Numeric("2".into()),
                    NumberComponent::Numeric("3".into()),
                ]
                .into(),
                letter: Some('a'),
            }
        );

        assert_eq!(
            VersionNumber::from_str("2.0.1").unwrap(),
            VersionNumber {
                components: [
                    NumberComponent::Numeric("2".into()),
                    NumberComponent::Numeric("0".into()),
                    NumberComponent::Numeric("1".into()),
                ]
                .into(),
                letter: None,
            }
        );

        assert_eq!(
            VersionNumber::from_str("1.2.03").unwrap(),
            VersionNumber {
                components: [
                    NumberComponent::Numeric("1".into()),
                    NumberComponent::Numeric("2".into()),
                    NumberComponent::Alphabetic("03".into()),
                ]
                .into(),
                letter: None,
            }
        );

        assert_eq!(
            VersionNumber::from_str("20251122").unwrap(),
            VersionNumber {
                components: [NumberComponent::Numeric("20251122".into())].into(),
                letter: None,
            }
        );
    }

    #[test]
    fn test_version_number_new_err() {
        let invalid_versions = vec!["", ".", "1..2", "1.2.3A", "1.2.3!"];
        for version in invalid_versions {
            assert!(
                VersionNumber::from_str(version).is_err(),
                "Version '{version}' should be invalid"
            );
        }
    }

    #[test]
    fn test_version_number_ord() {
        let v1_2_3 = VersionNumber::from_str("1.2.3").unwrap();
        let v1_2_03 = VersionNumber::from_str("1.2.03").unwrap();
        let v1_2_3a = VersionNumber::from_str("1.2.3a").unwrap();
        let v1_2_3b = VersionNumber::from_str("1.2.3b").unwrap();
        let v1_2_3_1 = VersionNumber::from_str("1.2.3.1").unwrap();
        let v1_2_4 = VersionNumber::from_str("1.2.4").unwrap();
        let v1_10_0 = VersionNumber::from_str("1.10.0").unwrap();
        let v1_10_0_1 = VersionNumber::from_str("1.10.0.1").unwrap();
        let v2_0 = VersionNumber::from_str("2.0").unwrap();
        let v2025_11_22 = VersionNumber::from_str("20251122").unwrap();

        assert!(v1_2_03 < v1_2_3);
        assert!(v1_2_3 < v1_2_4);
        assert!(v1_2_3 < v1_2_3a);
        assert!(v1_2_3a < v1_2_3b);
        assert!(v1_2_3b < v1_2_3_1);
        assert!(v1_2_3_1 < v1_2_4);
        assert!(v1_2_4 < v1_10_0);
        assert!(v1_10_0 < v1_10_0_1);
        assert!(v1_10_0_1 < v2_0);
        assert!(v2_0 < v2025_11_22);
    }

    #[test]
    fn test_version_number_display() {
        let test_cases = vec![
            (VersionNumber::from_str("1.2.3a").unwrap(), "1.2.3a"),
            (VersionNumber::from_str("2.0.1").unwrap(), "2.0.1"),
            (VersionNumber::from_str("1.2.03").unwrap(), "1.2.03"),
            (VersionNumber::from_str("20251122").unwrap(), "20251122"),
        ];
        for (version, expected) in test_cases {
            assert_eq!(version.to_string(), expected);
        }
    }

    #[test]
    fn test_number_component_ord() {
        let num1 = NumberComponent::Numeric("1".into());
        let num2 = NumberComponent::Numeric("2".into());
        let alpha03 = NumberComponent::Alphabetic("03".into());
        let alpha3 = NumberComponent::Alphabetic("3".into());

        assert!(num1 < num2);
        assert!(alpha03 < alpha3); // '03' vs '3' should compare as ascii
        assert!(alpha03 < num1); // Numeric vs Alphabetic comparison
        assert!(alpha3 > num2); // Alphabetic vs Numeric comparison
    }

    #[test]
    fn test_number_component_display() {
        let test_cases = vec![
            (NumberComponent::Numeric("2".into()), "2"),
            (NumberComponent::Alphabetic("03".into()), "03"),
            (NumberComponent::Numeric("3".into()), "3"),
        ];
        for (component, expected) in test_cases {
            assert_eq!(component.to_string(), expected);
        }
    }
}
