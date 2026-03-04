use anyhow::{Context, Result, anyhow};
use std::cmp::Ordering;
use std::str::FromStr;
use std::{fmt, hash};

const SUFFIX_PREFIXES: [&str; 5] = ["alpha", "beta", "pre", "rc", "p"];

/// Holds a list of [`VersionSuffix`] for a package version.
/// For example, `"_rc1_p20"`.
#[derive(Default, Clone, Eq, Debug)]
pub struct VersionSuffixes(Vec<VersionSuffix>);

impl VersionSuffixes {
    /// Returns an iterator over the contained [`VersionSuffix`].
    pub fn iter(&self) -> impl Iterator<Item = &VersionSuffix> {
        self.0.iter()
    }
}

impl FromStr for VersionSuffixes {
    type Err = anyhow::Error;

    /// Parses a string of version suffixes separated by underscores into a [`VersionSuffixes`] instance.
    /// `'_'` prefixes are ignored For example, `"_beta_p20230101"` is still valid.
    fn from_str(suffixes: &str) -> Result<Self> {
        let suffixes = suffixes
            .split('_')
            .filter(|s| !s.is_empty())
            .map(VersionSuffix::new)
            .collect::<Result<_>>()?;
        Ok(Self(suffixes))
    }
}

impl FromIterator<VersionSuffix> for VersionSuffixes {
    fn from_iter<T: IntoIterator<Item = VersionSuffix>>(iter: T) -> Self {
        let suffixes = iter.into_iter().collect();
        Self(suffixes)
    }
}

impl Ord for VersionSuffixes {
    /// Compares version suffixes. If one has an additional patch suffix, it's considered greater.
    fn cmp(&self, other: &Self) -> Ordering {
        match self.0.len().cmp(&other.0.len()) {
            Ordering::Equal => self
                .0
                .iter()
                .zip(other.0.iter())
                .map(|(a, b)| a.cmp(b))
                .find(|o| *o != Ordering::Equal)
                .unwrap_or(Ordering::Equal),
            Ordering::Greater => match self.0[other.0.len()] {
                VersionSuffix::Patch(_) => Ordering::Greater,
                _ => Ordering::Less,
            },
            Ordering::Less => match other.0[self.0.len()] {
                VersionSuffix::Patch(_) => Ordering::Less,
                _ => Ordering::Greater,
            },
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
#[derive(Clone, Eq, Debug)]
pub enum VersionSuffix {
    Alpha(Option<String>),
    Beta(Option<String>),
    Pre(Option<String>),
    Rc(Option<String>),
    Patch(Option<String>),
}

impl VersionSuffix {
    /// Creates a new [`VersionSuffix`] from the given suffix string.
    /// Must start with one of: "alpha", "beta", "pre", "rc", "p".
    pub fn new(suffix: &str) -> Result<Self> {
        if !SUFFIX_PREFIXES
            .iter()
            .any(|prefix| suffix.starts_with(prefix))
        {
            return Err(anyhow!("invalid version suffix: {suffix}"));
        }
        let split_index = suffix
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(suffix.len());
        let (suffix, number) = suffix.split_at(split_index);

        let number = if number.is_empty() {
            None
        } else {
            number
                .parse::<usize>()
                .with_context(|| anyhow!("unable to parse version suffix number: '{number}'"))?;
            Some(number.to_string())
        };

        let suffix = match suffix {
            "alpha" => Self::Alpha(number),
            "beta" => Self::Beta(number),
            "pre" => Self::Pre(number),
            "rc" => Self::Rc(number),
            "p" => Self::Patch(number),
            _ => return Err(anyhow!("invalid version suffix: {suffix}")),
        };
        Ok(suffix)
    }

    /// Deconstructs the suffix into its string representation and optional number.
    const fn deconstruct(&self) -> (&str, &Option<String>) {
        match self {
            VersionSuffix::Alpha(num) => ("alpha", num),
            VersionSuffix::Beta(num) => ("beta", num),
            VersionSuffix::Pre(num) => ("pre", num),
            VersionSuffix::Rc(num) => ("rc", num),
            VersionSuffix::Patch(num) => ("p", num),
        }
    }

    /// Returns the order of the suffix type for comparison purposes.
    const fn suffix_order(&self) -> usize {
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
        match self.suffix_order().cmp(&other.suffix_order()) {
            Ordering::Equal => {}
            non_equal => return non_equal,
        }

        match (self, other) {
            (VersionSuffix::Alpha(a), VersionSuffix::Alpha(b))
            | (VersionSuffix::Beta(a), VersionSuffix::Beta(b))
            | (VersionSuffix::Pre(a), VersionSuffix::Pre(b))
            | (VersionSuffix::Rc(a), VersionSuffix::Rc(b))
            | (VersionSuffix::Patch(a), VersionSuffix::Patch(b)) => match (a, b) {
                // Safe to unwrap since the number is guaranteed to be a valid usize if it exists
                (Some(a_num), Some(b_num)) => a_num
                    .parse::<usize>()
                    .unwrap()
                    .cmp(&b_num.parse::<usize>().unwrap()),
                (Some(_), None) => Ordering::Greater,
                (None, Some(_)) => Ordering::Less,
                (None, None) => Ordering::Equal,
            },
            _ => unreachable!(),
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
        self.suffix_order().hash(state);
        let num = match self {
            VersionSuffix::Alpha(n)
            | VersionSuffix::Beta(n)
            | VersionSuffix::Pre(n)
            | VersionSuffix::Rc(n)
            | VersionSuffix::Patch(n) => n,
        };
        num.hash(state);
    }
}

impl fmt::Display for VersionSuffix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (suffix, num) = self.deconstruct();
        write!(f, "{suffix}")?;
        if let Some(num) = num {
            write!(f, "{num}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_suffixes_from_str() {
        let test_cases = vec![
            ("", vec![]),
            ("_alpha", vec![VersionSuffix::Alpha(None)]),
            (
                "_alpha1_beta2",
                vec![
                    VersionSuffix::Alpha(Some("1".into())),
                    VersionSuffix::Beta(Some("2".into())),
                ],
            ),
            (
                "pre_rc_p20230101",
                vec![
                    VersionSuffix::Pre(None),
                    VersionSuffix::Rc(None),
                    VersionSuffix::Patch(Some("20230101".into())),
                ],
            ),
        ];
        for (input, expected) in test_cases {
            let suffixes = VersionSuffixes::from_str(input).unwrap();
            assert_eq!(suffixes.0, expected);
        }
    }

    #[test]
    fn test_version_suffix_new_ok() {
        let test_cases = vec![
            ("alpha", VersionSuffix::Alpha(None)),
            ("alpha1", VersionSuffix::Alpha(Some("1".into()))),
            ("beta", VersionSuffix::Beta(None)),
            ("beta2", VersionSuffix::Beta(Some("2".into()))),
            ("pre", VersionSuffix::Pre(None)),
            ("pre3", VersionSuffix::Pre(Some("3".into()))),
            ("rc", VersionSuffix::Rc(None)),
            ("rc4", VersionSuffix::Rc(Some("4".into()))),
            ("p", VersionSuffix::Patch(None)),
            ("p20230101", VersionSuffix::Patch(Some("20230101".into()))),
        ];
        for (input, expected) in test_cases {
            let suffix = VersionSuffix::new(input).unwrap();
            assert_eq!(suffix, expected);
        }
    }

    #[test]
    fn test_version_suffix_new_err() {
        let invalid_cases = vec!["alph", "betaa", "prex", "rc!", "patch1", "unknown", "p-1"];
        for input in invalid_cases {
            assert!(VersionSuffix::new(input).is_err());
        }
    }

    #[test]
    fn test_version_suffix_ord() {
        let alpha1 = VersionSuffix::Alpha(Some("1".into()));
        let alpha01 = VersionSuffix::Alpha(Some("01".into()));
        let alpha2 = VersionSuffix::Alpha(Some("2".into()));
        let beta_none = VersionSuffix::Beta(None);
        let beta1 = VersionSuffix::Beta(Some("1".into()));
        let pre1 = VersionSuffix::Pre(Some("1".into()));
        let rc_none = VersionSuffix::Rc(Some("1".into()));
        let patch1 = VersionSuffix::Patch(Some("1".into()));

        assert_eq!(alpha1, alpha01);
        assert!(alpha1 < alpha2);
        assert!(alpha2 < beta_none);
        assert!(beta_none < beta1);
        assert!(beta1 < pre1);
        assert!(pre1 < rc_none);
        assert!(rc_none < patch1);
    }

    #[test]
    fn test_version_suffix_display() {
        let test_cases = vec![
            (VersionSuffix::Alpha(Some("1".into())), "alpha1"),
            (VersionSuffix::Beta(None), "beta"),
            (VersionSuffix::Pre(Some("3".into())), "pre3"),
            (VersionSuffix::Rc(None), "rc"),
            (VersionSuffix::Patch(Some("20231231".into())), "p20231231"),
            (VersionSuffix::Patch(Some("01234".into())), "p01234"),
        ];
        for (suffix, expected) in test_cases {
            assert_eq!(suffix.to_string(), expected);
        }
    }
}
