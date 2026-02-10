use crate::package::Package;
use crate::package::version::PackageVersion;
use crate::regex::{ATOM_OP, CAT_PKG, CAT_PKG_VER_REV, REPOSITORY, SLOT_LOOSE};
use anyhow::{Result, anyhow};
use lazy_static::lazy_static;
use regex::{Captures, Regex};
use std::fmt;
use std::str::FromStr;

lazy_static! {
    /// Regex to capture simple atoms with category and package,
    /// optionally version, slot and repository e.g.: dev-lang/rust, dev-lang/rust-1.70.0.
    static ref ATOM_SIMPLE_RE: Regex = Regex::new(&format!(
        r"^{CAT_PKG}(?:\:(?P<slot>{SLOT_LOOSE}))?(?:\:\:(?P<repo>{REPOSITORY}))?$"
    ))
    .unwrap();
    /// Regex to capture atoms with operator, category, package,
    /// version, slot and repository e.g.: >=dev-lang/rust-1.70
    static ref ATOM_OPERATOR_RE: Regex = Regex::new(&format!(r"^{ATOM_OP}{CAT_PKG_VER_REV}$")).unwrap();
    /// Regex to capture atoms with '=' category, package and a version wildcard.
    static ref ATOM_STAR_RE: Regex = Regex::new(&format!(r"^={CAT_PKG_VER_REV}\*$")).unwrap();
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
enum Operator {
    Less,
    LessEqual,
    Equal,
    Greater,
    GreaterEqual,
    Approximate,
}

impl FromStr for Operator {
    type Err = anyhow::Error;

    fn from_str(operator: &str) -> Result<Self> {
        let op = match operator {
            "<" => Operator::Less,
            "<=" => Operator::LessEqual,
            "=" => Operator::Equal,
            ">" => Operator::Greater,
            ">=" => Operator::GreaterEqual,
            "~" => Operator::Approximate,
            _ => return Err(anyhow!("invalid operator: {operator}")),
        };
        Ok(op)
    }
}

impl fmt::Display for Operator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let op = match self {
            Operator::Less => "<",
            Operator::LessEqual => "<=",
            Operator::Equal => "=",
            Operator::Greater => ">",
            Operator::GreaterEqual => ">=",
            Operator::Approximate => "~",
        };
        write!(f, "{op}")
    }
}

#[derive(Default, Clone, Debug, Eq, Hash, PartialEq)]
pub enum AtomVariant {
    #[default]
    Simple,
    VersionOperator,
    VersionWildcard,
}

/// Represents a portage package atom. An atom is simply a dependency that is used by portage when
/// calculating relationships between packages.
/// TODO:
///  * implement remaining atom variants (see man 5 ebuild)
#[derive(Default, Clone, Debug, PartialEq, Hash, Eq)]
pub struct Atom {
    operator: Option<Operator>,
    category: String,
    package: String,
    version: Option<PackageVersion>,
    slot: Option<String>,
    repo: Option<String>,
    variant: AtomVariant,
}

impl Atom {
    /// Creates an Atom from the given string representation.
    /// Returns an error if the string is not a valid atom.
    pub fn new(atom: &str) -> Result<Self> {
        if let Some(caps) = ATOM_OPERATOR_RE.captures(atom) {
            return Ok(
                Self::from_regex_capture(&caps, AtomVariant::VersionOperator)?.with_operator(
                    match caps.name("operator") {
                        Some(m) => Some(Operator::from_str(m.as_str())?),
                        None => None,
                    },
                ),
            );
        }
        if let Some(caps) = ATOM_STAR_RE.captures(atom) {
            return Ok(
                Self::from_regex_capture(&caps, AtomVariant::VersionWildcard)?
                    .with_operator(Some(Operator::Equal)),
            );
        }
        if let Some(caps) = ATOM_SIMPLE_RE.captures(atom) {
            return Self::from_regex_capture(&caps, AtomVariant::Simple);
        }

        Err(anyhow!("'{atom}' is not a valid package atom"))
    }

    /// Returns the qualified name for this atom in the format category/name e.g. app-editors/vim.
    pub fn qualified_name(&self) -> String {
        format!("{}/{}", self.category, self.package)
    }

    /// Checks if the given `pkg` matches this atom.
    /// TODO: implement wildcard operator and slot matching
    pub fn matches(&self, pkg: &Package) -> bool {
        if self.category != pkg.category || self.package != pkg.name {
            return false;
        }
        if let Some(repo) = &self.repo
            && repo != &pkg.repo
        {
            return false;
        }

        self.matches_version(&pkg.version)
    }

    /// Checks if the given `pkg_ver` matches the this atoms [`PackageVersion`].
    fn matches_version(&self, pkg_ver: &PackageVersion) -> bool {
        let atom_ver = match &self.version {
            Some(v) => v,
            // If the atom doesn't specify a version, it matches any version
            None => return true,
        };
        match self.variant {
            AtomVariant::Simple => atom_ver == pkg_ver,
            AtomVariant::VersionOperator => match self.operator {
                Some(Operator::Less) => pkg_ver < atom_ver,
                Some(Operator::LessEqual) => pkg_ver <= atom_ver,
                Some(Operator::Equal) => {
                    // Requires equality of all defined components and suffixes
                    let components_match = pkg_ver
                        .number
                        .components
                        .iter()
                        .zip(&atom_ver.number.components)
                        .all(|(a, b)| a == b);
                    let suffixes_match = pkg_ver
                        .suffixes
                        .iter()
                        .zip(atom_ver.suffixes.iter())
                        .all(|(a, b)| a == b);
                    components_match && suffixes_match
                }
                Some(Operator::Greater) => pkg_ver > atom_ver,
                Some(Operator::GreaterEqual) => pkg_ver >= atom_ver,
                Some(Operator::Approximate) => {
                    pkg_ver.number == atom_ver.number && pkg_ver.suffixes == atom_ver.suffixes
                }
                None => unreachable!("BUG: atom is expected to have an operator"),
            },
            AtomVariant::VersionWildcard => todo!("wildcard matching not implemented"),
        }
    }

    /// Creates an Atom from the given regex captures.
    /// It assumes the correct regex has been used.
    /// NOTE: this does not set the operator field, see [`Self::with_operator`].
    fn from_regex_capture(caps: &Captures, variant: AtomVariant) -> Result<Self> {
        let version = match Self::parse_version(caps)? {
            Some(_) if variant == AtomVariant::Simple => Err(anyhow!(
                "atom must have an operator or be in format <category>/<package>"
            ))?,
            v => v,
        };

        Ok(Self {
            operator: None,
            category: caps
                .name("category")
                .ok_or_else(|| anyhow!("atom missing <category>"))?
                .as_str()
                .to_owned(),
            package: caps
                .name("package")
                .ok_or_else(|| anyhow!("atom missing <package>"))?
                .as_str()
                .to_owned(),
            version,
            slot: caps.name("slot").map(|m| m.as_str().to_owned()),
            repo: caps.name("repo").map(|m| m.as_str().to_owned()),
            variant,
        })
    }

    /// Parses the version including suffixes and revision from the given regex captures `caps`.
    /// If no version is found, returns `Ok(None)`.
    fn parse_version(caps: &Captures) -> Result<Option<PackageVersion>> {
        let version = match caps.name("version") {
            Some(m) => m.as_str(),
            None => return Ok(None),
        };
        let suffixes = caps.name("suffixes").map(|m| m.as_str());
        let revision = caps.name("revision").map(|m| m.as_str());

        Ok(Some(PackageVersion::new(version, suffixes, revision)?))
    }

    const fn with_operator(mut self, operator: Option<Operator>) -> Self {
        self.operator = operator;
        self
    }
}

impl FromStr for Atom {
    type Err = anyhow::Error;

    fn from_str(atom: &str) -> Result<Self> {
        Atom::new(atom)
    }
}

impl fmt::Display for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(op) = &self.operator {
            write!(f, "{op}")?;
        }
        write!(f, "{}/{}", self.category, self.package)?;
        if let Some(version) = &self.version {
            write!(f, "-{version}")?;
        }
        if let Some(slot) = &self.slot {
            write!(f, ":{slot}")?;
        }
        if let Some(repo) = &self.repo {
            write!(f, "::{repo}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atom_from_str_simple() {
        let test_cases = vec![
            (
                "dev-lang/rust",
                Atom {
                    category: "dev-lang".into(),
                    package: "rust".into(),
                    ..Default::default()
                },
            ),
            (
                "dev-lang/rust:1.92.0",
                Atom {
                    category: "dev-lang".into(),
                    package: "rust".into(),
                    slot: Some("1.92.0".into()),
                    ..Default::default()
                },
            ),
            (
                "net-misc/dhcp:0::gentoo",
                Atom {
                    category: "net-misc".into(),
                    package: "dhcp".into(),
                    slot: Some("0".into()),
                    repo: Some("gentoo".into()),
                    ..Default::default()
                },
            ),
            (
                "x11-drivers/nvidia-drivers:0/390",
                Atom {
                    category: "x11-drivers".into(),
                    package: "nvidia-drivers".into(),
                    slot: Some("0/390".into()),
                    ..Default::default()
                },
            ),
        ];

        for (atom_str, expected_atom) in test_cases {
            assert_eq!(Atom::new(atom_str).unwrap(), expected_atom);
        }
    }

    #[test]
    fn test_atom_from_str_operator() {
        let test_cases = vec![
            (
                "=sys-apps/memtest86+-7.2.0",
                Atom {
                    operator: Some(Operator::Equal),
                    category: "sys-apps".into(),
                    package: "memtest86+".into(),
                    version: PackageVersion::new("7.2.0", None, None).ok(),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                ">dev-lang/rust-1.70.0_beta-r2",
                Atom {
                    operator: Some(Operator::Greater),
                    category: "dev-lang".into(),
                    package: "rust".into(),
                    version: PackageVersion::new("1.70.0", Some("beta"), Some("2")).ok(),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                ">=sys-apps/sed-4.8",
                Atom {
                    operator: Some(Operator::GreaterEqual),
                    category: "sys-apps".into(),
                    package: "sed".into(),
                    version: PackageVersion::new("4.8", None, None).ok(),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                "<net-misc/dhcp-3",
                Atom {
                    operator: Some(Operator::Less),
                    category: "net-misc".into(),
                    package: "dhcp".into(),
                    version: PackageVersion::new("3", None, None).ok(),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                "<=net-misc/dhcp-3.0_p2",
                Atom {
                    operator: Some(Operator::LessEqual),
                    category: "net-misc".into(),
                    package: "dhcp".into(),
                    version: PackageVersion::new("3.0", Some("p2"), None).ok(),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                "~dev-lang/rust-1.70.0",
                Atom {
                    operator: Some(Operator::Approximate),
                    category: "dev-lang".into(),
                    package: "rust".into(),
                    version: PackageVersion::new("1.70.0", None, None).ok(),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
        ];

        for (atom_str, expected_atom) in test_cases {
            let atom = Atom::new(atom_str).unwrap();
            assert_eq!(atom, expected_atom);
        }
    }

    /// TODO: handle wildcards in version properly
    #[test]
    fn test_atom_from_str_star() {
        let test_cases = vec![
            (
                "=dev-lang/rust-1.70.0*",
                Atom {
                    operator: Some(Operator::Equal),
                    category: "dev-lang".into(),
                    package: "rust".into(),
                    version: PackageVersion::new("1.70.0", None, None).ok(),
                    variant: AtomVariant::VersionWildcard,
                    ..Default::default()
                },
            ),
            (
                "=dev-libs/glib-2*",
                Atom {
                    operator: Some(Operator::Equal),
                    category: "dev-libs".into(),
                    package: "glib".into(),
                    version: PackageVersion::new("2", None, None).ok(),
                    variant: AtomVariant::VersionWildcard,
                    ..Default::default()
                },
            ),
        ];

        for (atom_str, expected_atom) in test_cases {
            let atom = Atom::new(atom_str).unwrap();
            assert_eq!(atom, expected_atom);
        }
    }

    #[test]
    fn test_atom_from_str_err() {
        let invalid_atoms = vec![
            "invalid-atom",
            "dev-lang/",
            "/rust",
            ">=dev-lang/rust-",
            "dev-lang/rust-1.70.0",
            "=dev-lang/rust-1.70.0_extra",
            "dev-lang/rust:::",
            "dev-lang/rust*",
            "=dev-lang/rust*",
            "=dev-lang/rust-1.*",
        ];

        for atom_str in invalid_atoms {
            assert!(Atom::new(atom_str).is_err(), "{atom_str} should be invalid");
        }
    }

    #[test]
    fn test_atom_matches_true() {
        // TODO: test wildcards and slot matching
        let atoms = vec![
            "sys-devel/gcc",
            "sys-devel/gcc::gentoo",
            "=sys-devel/gcc-15",
            "=sys-devel/gcc-15.2",
            "=sys-devel/gcc-15.2.1",
            "=sys-devel/gcc-15.2.1_p20251122",
            "=sys-devel/gcc-15.2.1_p20251122-r1",
            ">sys-devel/gcc-15",
            ">=sys-devel/gcc-15.2.1",
            "<sys-devel/gcc-16",
            "<=sys-devel/gcc-15.2.2_p20260101",
            "~sys-devel/gcc-15.2.1_p20251122",
        ];
        let pkg = Package::new(
            "sys-devel",
            "gcc",
            PackageVersion::new("15.2.1", Some("p20251122"), Some("1")).unwrap(),
            "gentoo",
        )
        .unwrap();
        for atom in atoms {
            let atom = Atom::new(atom).unwrap();
            assert!(atom.matches(&pkg), "{atom} should match {pkg}");
        }
    }

    #[test]
    fn test_atom_matches_false() {
        let atoms = vec![
            "sys-devel/gcc::local",
            "sys-devel/binutils",
            "virtual/gcc",
            "<sys-devel/gcc-15",
            "<=sys-devel/gcc-15.2.1",
            ">sys-devel/gcc-16",
            ">=sys-devel/gcc-15.2.2_p20251122-r2",
            "=sys-devel/gcc-15.2.2",
            "=sys-devel/gcc-15.2.1_p20260330",
            "~sys-devel/gcc-15.3",
            "~sys-devel/gcc-15",
            "~sys-devel/gcc-15.2",
            "~sys-devel/gcc-15.2.1",
            "~sys-devel/gcc-15.2.1_p20260101",
        ];
        let pkg = Package::new(
            "sys-devel",
            "gcc",
            PackageVersion::new("15.2.1", Some("p20251122"), Some("1")).unwrap(),
            "gentoo",
        )
        .unwrap();
        for atom in atoms {
            let atom = Atom::new(atom).unwrap();
            assert!(!atom.matches(&pkg), "{atom} shouldn't match {pkg}");
        }
    }

    #[test]
    fn test_atom_qualified_name() {
        let atom = Atom {
            category: "dev-lang".into(),
            package: "rust".into(),
            ..Default::default()
        };
        assert_eq!(atom.qualified_name(), "dev-lang/rust");
    }

    #[test]
    fn test_atom_display() {
        let atom = Atom {
            operator: Some(Operator::GreaterEqual),
            category: "dev-lang".into(),
            package: "rust".into(),
            version: PackageVersion::new("1.70.0", Some("beta_p11"), Some("2")).ok(),
            slot: Some("1.70".into()),
            repo: Some("gentoo".into()),
            variant: AtomVariant::VersionOperator,
        };
        assert_eq!(
            atom.to_string(),
            ">=dev-lang/rust-1.70.0_beta_p11-r2:1.70::gentoo"
        );
    }

    #[test]
    fn test_operator_from_str_ok() {
        let test_cases = vec![
            ("<", Operator::Less),
            ("<=", Operator::LessEqual),
            ("=", Operator::Equal),
            (">", Operator::Greater),
            (">=", Operator::GreaterEqual),
            ("~", Operator::Approximate),
        ];
        for (op_str, expected_op) in test_cases {
            let op = Operator::from_str(op_str).unwrap();
            assert_eq!(op, expected_op);
        }
    }

    #[test]
    fn test_operator_from_str_err() {
        let invalid_ops = vec!["!", "==", "><", "=>", "invalid"];
        for op_str in invalid_ops {
            assert!(
                Operator::from_str(op_str).is_err(),
                "{op_str} should be invalid"
            );
        }
    }

    #[test]
    fn test_operator_display() {
        assert_eq!(Operator::Less.to_string(), "<");
        assert_eq!(Operator::LessEqual.to_string(), "<=");
        assert_eq!(Operator::Equal.to_string(), "=");
        assert_eq!(Operator::Greater.to_string(), ">");
        assert_eq!(Operator::GreaterEqual.to_string(), ">=");
        assert_eq!(Operator::Approximate.to_string(), "~");
    }
}
