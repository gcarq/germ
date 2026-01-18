use std::cmp::Ordering;
use std::fmt;
use std::hash::Hash;

/// Represents a package with its category, name, and available versions.
/// TODO: add slot and repo information.
#[derive(Eq, Debug)]
pub struct Package {
    category: String,
    name: String,
    version: PackageVersion,
}

impl Package {
    pub fn new(category: String, name: String, version: PackageVersion) -> Self {
        Self {
            category,
            name,
            version,
        }
    }

    /// Returns the qualified name of the package in the format category/name e.g. app-editors/vim.
    pub fn qualified_name(&self) -> String {
        format!("{}/{}", self.category, self.name)
    }
}

impl PartialEq<Self> for Package {
    fn eq(&self, other: &Self) -> bool {
        self.qualified_name() == other.qualified_name()
    }
}

impl Hash for Package {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.qualified_name().hash(state);
    }
}

impl fmt::Display for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.qualified_name())
    }
}

/// Represents a package version with its version string, suffixes, and revision.
#[derive(Eq, Debug, Default)]
pub struct PackageVersion {
    version: String,
    suffixes: Vec<PackageVersionSuffix>,
    revision: usize,
}

impl PackageVersion {
    pub fn new(version: String, suffixes: Vec<PackageVersionSuffix>, revision: usize) -> Self {
        debug_assert!(!version.is_empty());
        Self {
            version,
            suffixes,
            revision,
        }
    }

    /// Splits the version into its numeric components and an optional letter component.
    /// For example, "1.2.3a" becomes (["1", "2", "3"], Some('a')) and
    /// "2.0.1" becomes (["2", "0", "1"], None).
    fn version_components(&self) -> (Vec<String>, Option<char>) {
        let (version, letter) = match self.version.chars().last().unwrap() {
            c @ 'a'..='z' => (&self.version[..self.version.len() - 1], Some(c)),
            _ => (self.version.as_str(), None),
        };
        let components = version
            .split('.')
            .map(|part| part.to_string())
            .collect::<Vec<String>>();
        (components, letter)
    }
}

impl Ord for PackageVersion {
    /// Compares two package versions according to PMS section 3.3.
    fn cmp(&self, other: &Self) -> Ordering {
        let (a_components, a_letter) = self.version_components();
        let (b_comps, b_letter) = other.version_components();

        let mut a_comps_iter = a_components.iter();
        let mut b_comps_iter = b_comps.iter();

        // First numeric component uses int comparison. It's safe to unwrap as there must be at
        // least one component, and we know it only contains numbers.
        match a_comps_iter
            .next()
            .unwrap()
            .parse::<usize>()
            .unwrap()
            .cmp(&b_comps_iter.next().unwrap().parse::<usize>().unwrap())
        {
            Ordering::Equal => {}
            non_equal => return non_equal,
        }

        // Compare remaining numeric components
        while let (Some(a_comp), Some(b_comp)) = (a_comps_iter.next(), b_comps_iter.next()) {
            // If either component starts with '0', compare as ascii chars.
            if a_comp.starts_with("0") || b_comp.starts_with("0") {
                // Strip leading zeros for comparison.
                for (a_char, b_char) in a_comp
                    .trim_start_matches("0")
                    .chars()
                    .zip(b_comp.trim_start_matches("0").chars())
                {
                    match a_char.cmp(&b_char) {
                        Ordering::Equal => {}
                        non_equal => return non_equal,
                    }
                }
            } else {
                // Compare as integers, safe to unwrap as it can only contain numbers at this point.
                match a_comp
                    .parse::<usize>()
                    .unwrap()
                    .cmp(&b_comp.parse::<usize>().unwrap())
                {
                    Ordering::Equal => {}
                    non_equal => return non_equal,
                }
            }
        }

        // Compare letter component
        match (a_letter, b_letter) {
            (Some(a), Some(b)) => match a.cmp(&b) {
                Ordering::Equal => {}
                non_equal => return non_equal,
            },
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => {}
        }

        // Compare suffixes
        for (a_suffix, b_suffix) in self.suffixes.iter().zip(other.suffixes.iter()) {
            match a_suffix.cmp(b_suffix) {
                Ordering::Equal => {}
                non_equal => return non_equal,
            }
        }

        // If one has an additional patch suffix, its considered greater
        match self.suffixes.len().cmp(&other.suffixes.len()) {
            Ordering::Greater => {
                return match self.suffixes[other.suffixes.len()] {
                    PackageVersionSuffix::Patch(_) => Ordering::Greater,
                    _ => Ordering::Less,
                };
            }
            Ordering::Less => {
                return match other.suffixes[self.suffixes.len()] {
                    PackageVersionSuffix::Patch(_) => Ordering::Less,
                    _ => Ordering::Greater,
                };
            }
            Ordering::Equal => {}
        }

        // Compare package revision
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

impl Hash for PackageVersion {
    /// TODO: this must match the comparison logic,
    ///  this is currently not true for the version components starting with '0'.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.version.hash(state);
        for suffix in &self.suffixes {
            suffix.hash(state);
        }
        self.revision.hash(state);
    }
}

impl fmt::Display for PackageVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.version)?;
        for suffix in &self.suffixes {
            write!(f, "_{}", suffix)?;
        }
        if self.revision > 0 {
            write!(f, "-r{}", self.revision)?;
        }
        Ok(())
    }
}

/// Represents the different types of package version suffixes outlined in section 3.2.
#[derive(Eq, Debug)]
pub enum PackageVersionSuffix {
    Alpha(Option<usize>),
    Beta(Option<usize>),
    Pre(Option<usize>),
    Rc(Option<usize>),
    Patch(Option<usize>),
}

impl PackageVersionSuffix {
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
            PackageVersionSuffix::Alpha(_) => 0,
            PackageVersionSuffix::Beta(_) => 1,
            PackageVersionSuffix::Pre(_) => 2,
            PackageVersionSuffix::Rc(_) => 3,
            PackageVersionSuffix::Patch(_) => 4,
        }
    }
}

impl fmt::Display for PackageVersionSuffix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (suffix, num) = match self {
            PackageVersionSuffix::Alpha(n) => ("alpha", n),
            PackageVersionSuffix::Beta(n) => ("beta", n),
            PackageVersionSuffix::Pre(n) => ("pre", n),
            PackageVersionSuffix::Rc(n) => ("rc", n),
            PackageVersionSuffix::Patch(n) => ("p", n),
        };
        write!(f, "{suffix}")?;
        if let Some(num) = num {
            write!(f, "{num}")?;
        }
        Ok(())
    }
}

impl Ord for PackageVersionSuffix {
    /// Compares two package versions according to PMS section 3.3.
    fn cmp(&self, other: &Self) -> Ordering {
        match self.suffix_order().cmp(&other.suffix_order()) {
            Ordering::Equal => {}
            non_equal => return non_equal,
        }

        match (self, other) {
            (PackageVersionSuffix::Alpha(a), PackageVersionSuffix::Alpha(b))
            | (PackageVersionSuffix::Beta(a), PackageVersionSuffix::Beta(b))
            | (PackageVersionSuffix::Pre(a), PackageVersionSuffix::Pre(b))
            | (PackageVersionSuffix::Rc(a), PackageVersionSuffix::Rc(b))
            | (PackageVersionSuffix::Patch(a), PackageVersionSuffix::Patch(b)) => match (a, b) {
                (Some(a_num), Some(b_num)) => a_num.cmp(b_num),
                (Some(_), None) => Ordering::Greater,
                (None, Some(_)) => Ordering::Less,
                (None, None) => Ordering::Equal,
            },
            _ => unreachable!(),
        }
    }
}

impl PartialOrd for PackageVersionSuffix {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq<Self> for PackageVersionSuffix {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Hash for PackageVersionSuffix {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.suffix_order().hash(state);
        let num = match self {
            PackageVersionSuffix::Alpha(n)
            | PackageVersionSuffix::Beta(n)
            | PackageVersionSuffix::Pre(n)
            | PackageVersionSuffix::Rc(n)
            | PackageVersionSuffix::Patch(n) => n,
        };
        num.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_qualified_name() {
        let package = Package::new(
            "app-editors".into(),
            "vim".into(),
            PackageVersion::new("1.0.0".into(), vec![], 0),
        );
        assert_eq!(package.qualified_name(), "app-editors/vim");
    }

    #[test]
    fn test_package_version_display_simple() {
        assert_eq!(
            PackageVersion::new("1.0.0".into(), vec![], 0,).to_string(),
            "1.0.0"
        );
    }

    #[test]
    fn test_package_version_display_complex() {
        assert_eq!(
            PackageVersion::new(
                "2.3.4a".into(),
                vec![
                    PackageVersionSuffix::Alpha(Some(3)),
                    PackageVersionSuffix::Patch(Some(20250101))
                ],
                2,
            )
            .to_string(),
            "2.3.4a_alpha3_p20250101-r2"
        );
    }

    #[test]
    fn test_package_version_version_components() {
        let pv1 = PackageVersion::new("1.2.3a".into(), vec![], 0);
        assert_eq!(
            pv1.version_components(),
            (vec!["1".into(), "2".into(), "3".into()], Some('a'))
        );

        let pv2 = PackageVersion::new("2.0.1".into(), vec![], 0);
        assert_eq!(
            pv2.version_components(),
            (vec!["2".into(), "0".into(), "1".into()], None)
        );
    }

    #[test]
    fn test_package_version_ord_version() {
        let v1_2_3 = PackageVersion::new("1.2.3".into(), vec![], 0);
        let v1_2_4 = PackageVersion::new("1.2.4".into(), vec![], 0);
        let v1_10_0 = PackageVersion::new("1.10.0".into(), vec![], 0);
        let v1_2_03 = PackageVersion::new("1.2.03".into(), vec![], 0);
        let v_2_0 = PackageVersion::new("2.0".into(), vec![], 0);

        assert!(v1_2_3 < v1_2_4);
        assert!(v1_2_4 < v1_10_0);
        assert_eq!(v1_2_3, v1_2_03); // '03' vs '3' should compare as ascii
        assert!(v1_2_4 < v_2_0);
    }

    #[test]
    fn test_package_version_ord_letter() {
        let v1_2_3 = PackageVersion::new("1.2.3".into(), vec![], 0);
        let v1_2_3a = PackageVersion::new("1.2.3a".into(), vec![], 0);
        let v1_2_3b = PackageVersion::new("1.2.3b".into(), vec![], 0);

        assert!(v1_2_3 < v1_2_3a);
        assert!(v1_2_3a < v1_2_3b);
    }

    #[test]
    fn test_package_version_ord_suffixes() {
        let v1_2_3_alpha =
            PackageVersion::new("1.2.3".into(), vec![PackageVersionSuffix::Alpha(None)], 0);
        let v1_2_3_alpha_p2025 = PackageVersion::new(
            "1.2.3".into(),
            vec![
                PackageVersionSuffix::Alpha(None),
                PackageVersionSuffix::Patch(Some(2025)),
            ],
            0,
        );
        let v1_2_3_beta =
            PackageVersion::new("1.2.3".into(), vec![PackageVersionSuffix::Beta(None)], 0);
        let v1_2_3_patch =
            PackageVersion::new("1.2.3".into(), vec![PackageVersionSuffix::Patch(None)], 0);

        assert!(v1_2_3_alpha < v1_2_3_alpha_p2025);
        assert!(v1_2_3_alpha < v1_2_3_beta);
        assert!(v1_2_3_beta < v1_2_3_patch);
    }

    #[test]
    fn test_package_version_ord_revision() {
        let v1_r0 = PackageVersion::new("1.0.0".into(), vec![], 0);
        let v1_r1 = PackageVersion::new("1.0.0".into(), vec![], 1);
        let v1_r2 = PackageVersion::new("1.0.0".into(), vec![], 2);

        assert!(v1_r0 < v1_r1);
        assert!(v1_r1 < v1_r2);
    }

    #[test]
    fn test_package_version_ord_complex() {
        let v1_2_3a_p2024 = PackageVersion::new(
            "1.2.3a".into(),
            vec![
                PackageVersionSuffix::Alpha(None),
                PackageVersionSuffix::Patch(Some(20240101)),
            ],
            0,
        );
        let v1_2_3a_p2025 = PackageVersion::new(
            "1.2.3a".into(),
            vec![
                PackageVersionSuffix::Alpha(None),
                PackageVersionSuffix::Patch(Some(20251111)),
            ],
            0,
        );
        let v1_5_0b = PackageVersion::new("1.5.0b".into(), vec![], 0);

        assert!(v1_2_3a_p2024 < v1_2_3a_p2025);
        assert!(v1_2_3a_p2025 < v1_5_0b);
    }

    #[test]
    fn test_package_version_suffix_ord() {
        let alpha1 = PackageVersionSuffix::Alpha(Some(1));
        let alpha2 = PackageVersionSuffix::Alpha(Some(2));
        let beta_none = PackageVersionSuffix::Beta(None);
        let beta1 = PackageVersionSuffix::Beta(Some(1));
        let pre1 = PackageVersionSuffix::Pre(Some(1));
        let rc_none = PackageVersionSuffix::Rc(Some(1));
        let patch1 = PackageVersionSuffix::Patch(Some(1));

        assert!(alpha1 < alpha2);
        assert!(alpha2 < beta_none);
        assert!(beta_none < beta1);
        assert!(beta1 < pre1);
        assert!(pre1 < rc_none);
        assert!(rc_none < patch1);
    }

    #[test]
    fn test_package_version_suffix_display() {
        let alpha = PackageVersionSuffix::Alpha(Some(3));
        let beta = PackageVersionSuffix::Beta(None);
        let pre = PackageVersionSuffix::Pre(None);
        let rc = PackageVersionSuffix::Rc(Some(2));
        let p = PackageVersionSuffix::Patch(Some(20240101));

        assert_eq!(alpha.to_string(), "alpha3");
        assert_eq!(beta.to_string(), "beta");
        assert_eq!(pre.to_string(), "pre");
        assert_eq!(rc.to_string(), "rc2");
        assert_eq!(p.to_string(), "p20240101");
    }
}
