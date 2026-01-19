use anyhow::{Context, Result, anyhow};
use std::cmp::Ordering;
use std::str::FromStr;
use std::{fmt, hash};

const SUFFIX_PREFIXES: [&str; 5] = ["alpha", "beta", "pre", "rc", "p"];

/// Holds a list of version suffixes for a package version.
#[derive(Eq, Debug, Default)]
pub struct VersionSuffixes(Vec<VersionSuffix>);

impl FromStr for VersionSuffixes {
    type Err = anyhow::Error;

    /// Parses a string of version suffixes separated by underscores into a [`VersionSuffixes`] instance.
    /// '_' prefixes are ignored For example, "_beta_p20230101" is still valid.
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

/// Represents the different types of package version suffixes outlined in section 3.2.
#[derive(Eq, Debug)]
enum VersionSuffix {
    Alpha(Option<usize>),
    Beta(Option<usize>),
    Pre(Option<usize>),
    Rc(Option<usize>),
    Patch(Option<usize>),
}

impl VersionSuffix {
    /// Creates a new [`VersionSuffix`] from the given suffix string.
    /// Must start with one of: "alpha", "beta", "pre", "rc", "p".
    pub fn new(suffix: &str) -> Result<Self> {
        if !SUFFIX_PREFIXES
            .iter()
            .any(|prefix| suffix.starts_with(prefix))
        {
            return Err(anyhow!("invalid version suffix: {}", suffix));
        }
        let split_index = suffix
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(suffix.len());
        let (suffix, number) = suffix.split_at(split_index);
        let number_parsed =
            match number.is_empty() {
                true => None,
                false => Some(number.parse::<usize>().with_context(|| {
                    anyhow!("unable to parse version suffix number: {}", number)
                })?),
            };
        let suffix = match suffix {
            "alpha" => Self::Alpha(number_parsed),
            "beta" => Self::Beta(number_parsed),
            "pre" => Self::Pre(number_parsed),
            "rc" => Self::Rc(number_parsed),
            "p" => Self::Patch(number_parsed),
            _ => return Err(anyhow!("invalid version suffix: {suffix}")),
        };
        Ok(suffix)
    }

    /// Returns the order of the suffix type for comparison purposes.
    fn suffix_order(&self) -> usize {
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
                (Some(a_num), Some(b_num)) => a_num.cmp(b_num),
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
        let (suffix, num) = match self {
            VersionSuffix::Alpha(n) => ("alpha", n),
            VersionSuffix::Beta(n) => ("beta", n),
            VersionSuffix::Pre(n) => ("pre", n),
            VersionSuffix::Rc(n) => ("rc", n),
            VersionSuffix::Patch(n) => ("p", n),
        };
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
                vec![VersionSuffix::Alpha(Some(1)), VersionSuffix::Beta(Some(2))],
            ),
            (
                "pre_rc_p20230101",
                vec![
                    VersionSuffix::Pre(None),
                    VersionSuffix::Rc(None),
                    VersionSuffix::Patch(Some(20230101)),
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
            ("alpha1", VersionSuffix::Alpha(Some(1))),
            ("beta", VersionSuffix::Beta(None)),
            ("beta2", VersionSuffix::Beta(Some(2))),
            ("pre", VersionSuffix::Pre(None)),
            ("pre3", VersionSuffix::Pre(Some(3))),
            ("rc", VersionSuffix::Rc(None)),
            ("rc4", VersionSuffix::Rc(Some(4))),
            ("p", VersionSuffix::Patch(None)),
            ("p20230101", VersionSuffix::Patch(Some(20230101))),
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
        let alpha1 = VersionSuffix::Alpha(Some(1));
        let alpha2 = VersionSuffix::Alpha(Some(2));
        let beta_none = VersionSuffix::Beta(None);
        let beta1 = VersionSuffix::Beta(Some(1));
        let pre1 = VersionSuffix::Pre(Some(1));
        let rc_none = VersionSuffix::Rc(Some(1));
        let patch1 = VersionSuffix::Patch(Some(1));

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
            (VersionSuffix::Alpha(Some(1)), "alpha1"),
            (VersionSuffix::Beta(None), "beta"),
            (VersionSuffix::Pre(Some(3)), "pre3"),
            (VersionSuffix::Rc(None), "rc"),
            (VersionSuffix::Patch(Some(20231231)), "p20231231"),
        ];
        for (suffix, expected) in test_cases {
            assert_eq!(suffix.to_string(), expected);
        }
    }
}
