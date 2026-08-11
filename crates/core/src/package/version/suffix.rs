use super::numeric::NumericComponent;
use anyhow::{Context, anyhow, bail};
use rkyv::{Archive, Deserialize, Serialize};
use std::cmp::Ordering;
use std::str::FromStr;
use std::{fmt, hash};

const SUFFIX_PREFIXES: [&str; 5] = ["alpha", "beta", "pre", "rc", "p"];

/// Holds a list of [`VersionSuffix`] for a package version.
/// For example, `"_rc1_p20"`.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, Default, Debug)]
pub struct VersionSuffixes(Box<[VersionSuffix]>);

impl VersionSuffixes {
    /// Parses a string of version suffixes separated by underscores into a [`VersionSuffixes`] instance.
    ///
    /// The initial `_` is required, e.g.: `"_beta_p20230101"`.
    fn new(suffixes: &str) -> anyhow::Result<Self> {
        if suffixes.is_empty() {
            return Ok(Self::default());
        }
        let suffixes = suffixes
            .strip_prefix('_')
            .ok_or_else(|| anyhow!("suffixes must start with '_'"))?
            .split('_')
            .map(|suffix| match suffix.is_empty() {
                true => bail!("empty version suffix"),
                false => VersionSuffix::new(suffix),
            })
            .collect::<anyhow::Result<_>>()?;
        Ok(Self(suffixes))
    }

    pub fn iter(&self) -> impl Iterator<Item = &VersionSuffix> {
        self.0.iter()
    }
}

impl FromStr for VersionSuffixes {
    type Err = anyhow::Error;

    fn from_str(suffixes: &str) -> anyhow::Result<Self> {
        Self::new(suffixes)
    }
}

impl FromIterator<VersionSuffix> for VersionSuffixes {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = VersionSuffix>,
    {
        let suffixes = iter.into_iter().collect();
        Self(suffixes)
    }
}

impl Ord for VersionSuffixes {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare shared suffixes first
        for (a, b) in self.0.iter().zip(&other.0) {
            match a.cmp(b) {
                Ordering::Equal => {}
                ordering => return ordering,
            }
        }

        // At this point they have a different length, so select the next suffix
        // for each (if any) and compare them according to PMS 3.5.
        match (self.0.get(other.0.len()), other.0.get(self.0.len())) {
            (Some(VersionSuffix::Patch(_)), None) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (None, Some(VersionSuffix::Patch(_))) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
            (Some(_), Some(_)) => unreachable!(),
        }
    }
}

impl PartialOrd for VersionSuffixes {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq<Self> for VersionSuffixes {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl hash::Hash for VersionSuffixes {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl fmt::Display for VersionSuffixes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for suffix in &self.0 {
            write!(f, "_{suffix}")?;
        }
        Ok(())
    }
}

/// Represents the different package version suffixes outlined in section 3.2.
/// For example: `"alpha1", "beta2", "pre", "rc", "p20230101"`.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, Debug)]
pub enum VersionSuffix {
    Alpha(Option<NumericComponent>),
    Beta(Option<NumericComponent>),
    Pre(Option<NumericComponent>),
    Rc(Option<NumericComponent>),
    Patch(Option<NumericComponent>),
}

impl VersionSuffix {
    /// Creates a new [`VersionSuffix`] from the given suffix string.
    /// Must start with one of: "alpha", "beta", "pre", "rc", "p".
    pub fn new(suffix: &str) -> anyhow::Result<Self> {
        if !SUFFIX_PREFIXES
            .iter()
            .any(|prefix| suffix.starts_with(prefix))
        {
            bail!("invalid version suffix: {suffix}");
        }
        let split_index = suffix
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(suffix.len());
        let (suffix, number) = suffix.split_at(split_index);

        let number = match number.is_empty() {
            true => None,
            false => Some(
                number
                    .parse()
                    .with_context(|| anyhow!("unable to parse version suffix"))?,
            ),
        };

        let suffix = match suffix {
            "alpha" => Self::Alpha(number),
            "beta" => Self::Beta(number),
            "pre" => Self::Pre(number),
            "rc" => Self::Rc(number),
            "p" => Self::Patch(number),
            _ => bail!("invalid version suffix: {suffix}"),
        };
        Ok(suffix)
    }

    pub const fn name(&self) -> &'static str {
        self.deconstruct().0
    }

    pub const fn number(&self) -> Option<&NumericComponent> {
        self.deconstruct().1.as_ref()
    }

    /// Deconstructs the suffix into its string representation and optional number.
    const fn deconstruct(&self) -> (&'static str, &Option<NumericComponent>) {
        match self {
            VersionSuffix::Alpha(num) => ("alpha", num),
            VersionSuffix::Beta(num) => ("beta", num),
            VersionSuffix::Pre(num) => ("pre", num),
            VersionSuffix::Rc(num) => ("rc", num),
            VersionSuffix::Patch(num) => ("p", num),
        }
    }

    /// Returns the order of the suffix type for comparison purposes.
    const fn ordinal(&self) -> usize {
        match self {
            VersionSuffix::Alpha(_) => 0,
            VersionSuffix::Beta(_) => 1,
            VersionSuffix::Pre(_) => 2,
            VersionSuffix::Rc(_) => 3,
            VersionSuffix::Patch(_) => 4,
        }
    }
}

impl Ord for VersionSuffix {
    /// Compares two package versions according to PMS section 3.3.
    fn cmp(&self, other: &Self) -> Ordering {
        match self.ordinal().cmp(&other.ordinal()) {
            Ordering::Equal => (),
            non_equal => return non_equal,
        }

        match (self.deconstruct().1, other.deconstruct().1) {
            // Zero is considered equal to None
            (Some(a), None) if a.is_zero() => Ordering::Equal,
            (None, Some(b)) if b.is_zero() => Ordering::Equal,
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            (None, None) => Ordering::Equal,
        }
    }
}

impl PartialOrd for VersionSuffix {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq<Self> for VersionSuffix {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl hash::Hash for VersionSuffix {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        let (suffix, number) = self.deconstruct();
        suffix.hash(state);
        number
            .as_ref()
            .map(NumericComponent::normalized)
            .unwrap_or_default()
            .hash(state);
    }
}

impl fmt::Display for VersionSuffix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (suffix, num) = self.deconstruct();
        f.write_str(suffix)?;
        if let Some(num) = num {
            f.write_str(num.as_str())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::assert_eq_hash;

    #[test]
    fn test_version_suffixes_parse() {
        let test_cases = [
            ("", [].into()),
            ("_alpha", [VersionSuffix::Alpha(None)].into()),
            (
                "_alpha1_beta2",
                [
                    VersionSuffix::Alpha(Some("1".parse().unwrap())),
                    VersionSuffix::Beta(Some("2".parse().unwrap())),
                ]
                .into(),
            ),
            (
                "_pre_rc_p20230101",
                [
                    VersionSuffix::Pre(None),
                    VersionSuffix::Rc(None),
                    VersionSuffix::Patch(Some("20230101".parse().unwrap())),
                ]
                .into(),
            ),
        ];
        for (input, expected) in test_cases {
            let suffixes = input.parse::<VersionSuffixes>().unwrap();
            assert_eq_hash(&suffixes.0, &expected);
        }
    }

    #[test]
    fn test_version_suffixes_parse_error() {
        assert!(VersionSuffixes::from_str("_alpha__beta").is_err());
    }

    #[test]
    fn test_version_suffix_parse() {
        let test_cases = [
            ("alpha", VersionSuffix::Alpha(None)),
            ("alpha1", VersionSuffix::Alpha(Some("1".parse().unwrap()))),
            ("beta", VersionSuffix::Beta(None)),
            ("beta2", VersionSuffix::Beta(Some("2".parse().unwrap()))),
            ("pre", VersionSuffix::Pre(None)),
            ("pre3", VersionSuffix::Pre(Some("3".parse().unwrap()))),
            ("rc", VersionSuffix::Rc(None)),
            ("rc4", VersionSuffix::Rc(Some("4".parse().unwrap()))),
            ("p", VersionSuffix::Patch(None)),
            (
                "p20230101",
                VersionSuffix::Patch(Some("20230101".parse().unwrap())),
            ),
            (
                "alpha999999999999999999999999",
                VersionSuffix::Alpha(Some("999999999999999999999999".parse().unwrap())),
            ),
        ];
        for (input, expected) in test_cases {
            let suffix = VersionSuffix::new(input).unwrap();
            assert_eq_hash(&suffix, &expected);
        }
    }

    #[test]
    fn test_version_suffix_parse_error() {
        let invalid_cases = ["alph", "betaa", "prex", "rc!", "patch1", "unknown", "p-1"];
        for input in invalid_cases {
            assert!(VersionSuffix::new(input).is_err());
        }
    }

    #[test]
    fn test_version_suffix_equality() {
        assert_eq_hash(
            &VersionSuffix::Alpha(None),
            &VersionSuffix::Alpha(Some("0".parse().unwrap())),
        );
    }

    #[test]
    fn test_version_suffix_display() {
        let test_cases = [
            (VersionSuffix::Alpha(Some("1".parse().unwrap())), "alpha1"),
            (VersionSuffix::Beta(None), "beta"),
            (VersionSuffix::Pre(Some("3".parse().unwrap())), "pre3"),
            (VersionSuffix::Rc(None), "rc"),
            (
                VersionSuffix::Patch(Some("20231231".parse().unwrap())),
                "p20231231",
            ),
            (
                VersionSuffix::Patch(Some("01234".parse().unwrap())),
                "p01234",
            ),
            (
                VersionSuffix::Alpha(Some("999999999999999999999999".parse().unwrap())),
                "alpha999999999999999999999999",
            ),
        ];
        for (suffix, expected) in test_cases {
            assert_eq!(suffix.to_string(), expected);
        }
    }
}
