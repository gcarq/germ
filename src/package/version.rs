use anyhow::{Result, anyhow};
use std::cmp::Ordering;
use std::{fmt, hash};

/// Represents a package version according to PMS section 3.2 and 3.3.
/// This includes the base version components (e.g., "1.2.3a"), any suffixes
/// (e.g., "_alpha1", "_p20240101"), and the revision number (e.g., "-r1").
#[derive(Eq, Debug)]
pub struct PackageVersion {
    pub number: VersionNumber,
    pub suffixes: Vec<VersionSuffix>,
    pub revision: usize,
}

impl PackageVersion {
    /// Creates a new [`PackageVersion`] from the given version string, suffixes, and revision.
    pub fn new(version: String, suffixes: Vec<VersionSuffix>, revision: usize) -> Result<Self> {
        debug_assert!(!version.is_empty());
        Ok(Self {
            number: VersionNumber::new(&version)?,
            suffixes,
            revision,
        })
    }

    /// Compares version suffixes. If one has an additional patch suffix, it's considered greater.
    pub fn cmp_suffixes(&self, other: &Self) -> Ordering {
        match self.suffixes.len().cmp(&other.suffixes.len()) {
            Ordering::Equal => self
                .suffixes
                .iter()
                .zip(other.suffixes.iter())
                .map(|(a, b)| a.cmp(b))
                .find(|o| *o != Ordering::Equal)
                .unwrap_or(Ordering::Equal),
            Ordering::Greater => match self.suffixes[other.suffixes.len()] {
                VersionSuffix::Patch(_) => Ordering::Greater,
                _ => Ordering::Less,
            },
            Ordering::Less => match other.suffixes[self.suffixes.len()] {
                VersionSuffix::Patch(_) => Ordering::Less,
                _ => Ordering::Greater,
            },
        }
    }
}

impl Ord for PackageVersion {
    /// Compares two package versions according to PMS section 3.3.
    fn cmp(&self, other: &Self) -> Ordering {
        match self.number.cmp(&other.number) {
            Ordering::Equal => {}
            non_equal => return non_equal,
        }
        match self.cmp_suffixes(other) {
            Ordering::Equal => {}
            non_equal => return non_equal,
        }
        self.revision.cmp(&other.revision)
    }
}

impl PartialOrd for PackageVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq<Self> for PackageVersion {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl hash::Hash for PackageVersion {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.number.hash(state);
        self.suffixes.hash(state);
        self.revision.hash(state);
    }
}

impl fmt::Display for PackageVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.number)?;
        for suffix in &self.suffixes {
            write!(f, "_{}", suffix)?;
        }
        if self.revision > 0 {
            write!(f, "-r{}", self.revision)?;
        }
        Ok(())
    }
}

/// Represents the base version number as individual components and an optional letter suffix.
#[derive(Eq, Debug)]
pub struct VersionNumber {
    pub components: Vec<BaseVersionComponent>,
    pub letter: Option<char>,
}

impl VersionNumber {
    /// Splits the version into its numeric components and an optional letter suffix.
    /// For example, "1.2.3a" becomes (["1", "2", "3", "a"], Some('a')),
    /// "2.0.1" becomes (["2", "0", "1"], None).
    fn new(version: &str) -> Result<Self> {
        // Safe to unwrap because the base version contains at least one character
        let (version, letter) = match version
            .chars()
            .last()
            .ok_or_else(|| anyhow!("unable to parse version number from: {version}"))?
        {
            c @ 'a'..='z' => (&version[..version.len() - 1], Some(c)),
            _ => (version, None),
        };
        let components = version
            .split('.')
            .map(|part| part.to_string())
            .enumerate()
            .map(|(i, part)| match i {
                0 => BaseVersionComponent::Alphabetic(part),
                _ if part.starts_with('0') => BaseVersionComponent::Alphabetic(part),
                _ => BaseVersionComponent::Numeric(part),
            })
            .collect();
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
        let repr = self
            .components
            .iter()
            .map(|comp| comp.to_string())
            .collect::<Vec<String>>()
            .join(".");
        write!(f, "{repr}")?;
        if let Some(letter) = self.letter {
            write!(f, "{letter}")?;
        }
        Ok(())
    }
}

/// Represents a component of the base version, either numeric or alphabetic.
/// E.g., in "1.2.03a", "1" and "2" are Numeric, "03" is Alphabetic, and "a" is handled separately
/// and not part of this enum.
#[derive(Eq, Debug)]
pub enum BaseVersionComponent {
    Numeric(String),
    Alphabetic(String),
}

impl Ord for BaseVersionComponent {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            // Compare as integers, safe to unwrap as it can only contain numbers at this point.
            (BaseVersionComponent::Numeric(a), BaseVersionComponent::Numeric(b)) => a
                .parse::<usize>()
                .unwrap()
                .cmp(&b.parse::<usize>().unwrap()),
            (BaseVersionComponent::Alphabetic(a), BaseVersionComponent::Alphabetic(b))
            | (BaseVersionComponent::Numeric(a), BaseVersionComponent::Alphabetic(b))
            | (BaseVersionComponent::Alphabetic(a), BaseVersionComponent::Numeric(b)) => {
                a.trim_start_matches('0').cmp(b.trim_start_matches('0'))
            }
        }
    }
}

impl PartialEq<Self> for BaseVersionComponent {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl PartialOrd<Self> for BaseVersionComponent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for BaseVersionComponent {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        let comp = match self {
            BaseVersionComponent::Numeric(n) => n,
            BaseVersionComponent::Alphabetic(a) => a.trim_start_matches('0'),
        };
        comp.hash(state);
    }
}

impl fmt::Display for BaseVersionComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let repr = match self {
            BaseVersionComponent::Numeric(n) => n,
            BaseVersionComponent::Alphabetic(a) => a,
        };
        write!(f, "{repr}")
    }
}

/// Represents the different types of package version suffixes outlined in section 3.2.
#[derive(Eq, Debug)]
pub enum VersionSuffix {
    Alpha(Option<usize>),
    Beta(Option<usize>),
    Pre(Option<usize>),
    Rc(Option<usize>),
    Patch(Option<usize>),
}

impl VersionSuffix {
    pub fn new(suffix: &str) -> Self {
        debug_assert!(
            ["alpha", "beta", "pre", "rc", "p"]
                .iter()
                .any(|&prefix| suffix.starts_with(prefix))
        );
        let split_index = suffix
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(suffix.len());
        let (suffix, number) = suffix.split_at(split_index);
        let number_parsed = match number.is_empty() {
            true => None,
            false => number.parse::<usize>().ok(),
        };
        match suffix {
            "alpha" => Self::Alpha(number_parsed),
            "beta" => Self::Beta(number_parsed),
            "pre" => Self::Pre(number_parsed),
            "rc" => Self::Rc(number_parsed),
            "p" => Self::Patch(number_parsed),
            _ => unreachable!(),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_version_display_simple() {
        assert_eq!(
            PackageVersion::new("1.0.0".into(), vec![], 0,)
                .unwrap()
                .to_string(),
            "1.0.0"
        );
    }

    #[test]
    fn test_package_version_display_complex() {
        assert_eq!(
            PackageVersion::new(
                "2.3.4a".into(),
                vec![
                    VersionSuffix::Alpha(Some(3)),
                    VersionSuffix::Patch(Some(20250101))
                ],
                2,
            )
            .unwrap()
            .to_string(),
            "2.3.4a_alpha3_p20250101-r2"
        );
    }

    #[test]
    fn test_version_components_new() {
        assert_eq!(
            VersionNumber::new("1.2.3a").unwrap(),
            VersionNumber {
                components: vec![
                    BaseVersionComponent::Numeric("1".into()),
                    BaseVersionComponent::Numeric("2".into()),
                    BaseVersionComponent::Numeric("3".into()),
                ],
                letter: Some('a'),
            }
        );

        assert_eq!(
            VersionNumber::new("2.0.1").unwrap(),
            VersionNumber {
                components: vec![
                    BaseVersionComponent::Numeric("2".into()),
                    BaseVersionComponent::Numeric("0".into()),
                    BaseVersionComponent::Numeric("1".into()),
                ],
                letter: None,
            }
        );

        assert_eq!(
            VersionNumber::new("1.2.03").unwrap(),
            VersionNumber {
                components: vec![
                    BaseVersionComponent::Numeric("1".into()),
                    BaseVersionComponent::Numeric("2".into()),
                    BaseVersionComponent::Alphabetic("03".into()),
                ],
                letter: None,
            }
        );

        assert_eq!(
            VersionNumber::new("20251122").unwrap(),
            VersionNumber {
                components: vec![BaseVersionComponent::Numeric("20251122".into()),],
                letter: None,
            }
        );
    }

    #[test]
    fn test_package_version_ord_version() {
        let v1_2_3 = PackageVersion::new("1.2.3".into(), vec![], 0).unwrap();
        let v1_2_03 = PackageVersion::new("1.2.03".into(), vec![], 0).unwrap();
        let v1_2_4 = PackageVersion::new("1.2.4".into(), vec![], 0).unwrap();
        let v1_10_0 = PackageVersion::new("1.10.0".into(), vec![], 0).unwrap();
        let v1_10_0_1 = PackageVersion::new("1.10.0.1".into(), vec![], 0).unwrap();
        let v2_0 = PackageVersion::new("2.0".into(), vec![], 0).unwrap();
        let v2025_11_22 = PackageVersion::new("20251122".into(), vec![], 0).unwrap();

        assert!(v1_2_3 < v1_2_4);
        assert_eq!(v1_2_3, v1_2_03); // '03' vs '3' should compare as ascii
        assert!(v1_2_4 < v1_10_0);
        assert!(v1_10_0 < v1_10_0_1);
        assert!(v1_10_0_1 < v2_0);
        assert!(v2_0 < v2025_11_22);
    }

    #[test]
    fn test_package_version_ord_letter() {
        let v1_2_3 = PackageVersion::new("1.2.3".into(), vec![], 0).unwrap();
        let v1_2_3a = PackageVersion::new("1.2.3a".into(), vec![], 0).unwrap();
        let v1_2_3b = PackageVersion::new("1.2.3b".into(), vec![], 0).unwrap();
        let v1_2_3_1 = PackageVersion::new("1.2.3.1".into(), vec![], 0).unwrap();

        assert!(v1_2_3 < v1_2_3a);
        assert!(v1_2_3a < v1_2_3b);
        assert!(v1_2_3b < v1_2_3_1);
    }

    #[test]
    fn test_package_version_ord_suffixes() {
        let v1_2_3_alpha =
            PackageVersion::new("1.2.3".into(), vec![VersionSuffix::Alpha(None)], 0).unwrap();
        let v1_2_3_alpha_p2025 = PackageVersion::new(
            "1.2.3".into(),
            vec![VersionSuffix::Alpha(None), VersionSuffix::Patch(Some(2025))],
            0,
        )
        .unwrap();
        let v1_2_3_beta =
            PackageVersion::new("1.2.3".into(), vec![VersionSuffix::Beta(None)], 0).unwrap();
        let v1_2_3_patch =
            PackageVersion::new("1.2.3".into(), vec![VersionSuffix::Patch(None)], 0).unwrap();

        assert!(v1_2_3_alpha < v1_2_3_alpha_p2025);
        assert!(v1_2_3_alpha < v1_2_3_beta);
        assert!(v1_2_3_beta < v1_2_3_patch);
    }

    #[test]
    fn test_package_version_ord_revision() {
        let v1_r0 = PackageVersion::new("1.0.0".into(), vec![], 0).unwrap();
        let v1_r1 = PackageVersion::new("1.0.0".into(), vec![], 1).unwrap();
        let v1_r2 = PackageVersion::new("1.0.0".into(), vec![], 2).unwrap();

        assert!(v1_r0 < v1_r1);
        assert!(v1_r1 < v1_r2);
    }

    #[test]
    fn test_package_version_ord_complex() {
        let v1_2_3a_p2024 = PackageVersion::new(
            "1.2.3a".into(),
            vec![
                VersionSuffix::Alpha(None),
                VersionSuffix::Patch(Some(20240101)),
            ],
            0,
        )
        .unwrap();
        let v1_2_3a_p2025 = PackageVersion::new(
            "1.2.3a".into(),
            vec![
                VersionSuffix::Alpha(None),
                VersionSuffix::Patch(Some(20251111)),
            ],
            0,
        )
        .unwrap();
        let v1_5_0b = PackageVersion::new("1.5.0b".into(), vec![], 0).unwrap();

        assert!(v1_2_3a_p2024 < v1_2_3a_p2025);
        assert!(v1_2_3a_p2025 < v1_5_0b);
    }

    #[test]
    fn test_package_version_suffix_ord() {
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
    fn test_package_version_suffix_display() {
        let alpha = VersionSuffix::Alpha(Some(3));
        let beta = VersionSuffix::Beta(None);
        let pre = VersionSuffix::Pre(None);
        let rc = VersionSuffix::Rc(Some(2));
        let p = VersionSuffix::Patch(Some(20240101));

        assert_eq!(alpha.to_string(), "alpha3");
        assert_eq!(beta.to_string(), "beta");
        assert_eq!(pre.to_string(), "pre");
        assert_eq!(rc.to_string(), "rc2");
        assert_eq!(p.to_string(), "p20240101");
    }
}
