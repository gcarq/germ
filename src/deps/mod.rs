use crate::package::Package;
use crate::package::slot::PackageSlot;
use crate::package::version::PackageVersion;
use crate::regex::{CATEGORY, PACKAGE, PV_REV, REPOSITORY, V_REV};
use anyhow::{Result, anyhow};
use constcat::concat;
use regex::{Captures, Regex};
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

/// Captures atom operators.
const ATOM_OPERATOR: &str = r"(?<operator>[=~]|[><]=?)";

/// Captures category and package with optional version and revision.
const ATOM_CP: &str = concat!(CATEGORY, "/", PACKAGE, "(?:-", V_REV, ")?");

/// Captures category, package, version and revision.
const ATOM_CPV_REV: &str = concat!(CATEGORY, "/", PV_REV);

/// Captures optional slot information in atoms.
const ATOM_SLOT_LOOSE: &str = r"(?:\:(?P<slot>([a-zA-Z0-9_+./*=-]+)))?";

/// Captures optional repository information in atoms.
const ATOM_REPOSITORY: &str = concat!(r"(?:\:\:", REPOSITORY, ")?");

/// Regex to capture simple atoms with category and package,
/// optionally version, slot and repository e.g.: `dev-lang/rust-1.70.0`.
static ATOM_SIMPLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^{ATOM_CP}{ATOM_SLOT_LOOSE}{ATOM_REPOSITORY}$")).unwrap()
});

/// Regex to capture atoms with operator, category, package,
/// version, ... e.g.: `>=dev-lang/rust-1.70`
static ATOM_OPERATOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^{ATOM_OPERATOR}{ATOM_CPV_REV}{ATOM_SLOT_LOOSE}{ATOM_REPOSITORY}$"
    ))
    .unwrap()
});

/// Regex to capture atoms with an equal operator, category, package and a version wildcard, ...
/// e.g.: `=dev-lang/rust-1.70*`
static ATOM_WILDCARD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^={ATOM_CPV_REV}\*{ATOM_SLOT_LOOSE}{ATOM_REPOSITORY}$"
    ))
    .unwrap()
});

/// Specifies the operator of an atom, which determines how packages are matched.
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

/// Specifies the variant of an atom, which determines how packages are matched.
#[derive(Default, Clone, Debug, Eq, Hash, PartialEq)]
enum Variant {
    #[default]
    Simple,
    VersionOperator,
    VersionWildcard,
}

/// Represents a portage package atom.
///
/// An atom can match one or more [`Package`] and is used for
/// calculating dependencies between packages.
/// TODO:
///  * implement remaining atom variants (see man 5 ebuild)
#[derive(Default, Clone, Debug, PartialEq, Hash, Eq)]
pub struct Atom {
    operator: Option<Operator>,
    category: String,
    package: String,
    version: Option<PackageVersion>,
    slot: Option<PackageSlot>,
    repo: Option<String>,
    variant: Variant,
}

impl Atom {
    /// Creates an [`Atom`] from the given `atom` string.
    ///
    /// Returns `Err` if the string is not a valid atom.
    pub fn new(atom: &str) -> Result<Self> {
        if let Some(caps) = ATOM_OPERATOR_RE.captures(atom) {
            return Ok(
                Self::from_regex_capture(&caps, Variant::VersionOperator)?.with_operator(
                    match caps.name("operator") {
                        Some(m) => Some(Operator::from_str(m.as_str())?),
                        None => None,
                    },
                ),
            );
        }
        if let Some(caps) = ATOM_WILDCARD_RE.captures(atom) {
            return Ok(Self::from_regex_capture(&caps, Variant::VersionWildcard)?
                .with_operator(Some(Operator::Equal)));
        }
        if let Some(caps) = ATOM_SIMPLE_RE.captures(atom) {
            return Self::from_regex_capture(&caps, Variant::Simple);
        }

        Err(anyhow!("'{atom}' is not a valid package atom"))
    }

    /// Returns the qualified name for this atom in the format
    /// `category/name` e.g. `app-editors/vim`.
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
        let Some(atom_ver) = &self.version else {
            // If the atom doesn't specify a version, it matches any version
            return true;
        };
        match self.variant {
            Variant::Simple => atom_ver == pkg_ver,
            Variant::VersionOperator => match self.operator {
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
            Variant::VersionWildcard => todo!("wildcard matching not implemented"),
        }
    }

    /// Creates an Atom from the given regex `captures` and `variant`.
    ///
    /// It assumes the correct regex has been used.
    /// NOTE: this does not set the operator field, see [`Self::with_operator`].
    fn from_regex_capture(captures: &Captures, variant: Variant) -> Result<Self> {
        let version = match Self::parse_version(captures)? {
            Some(_) if variant == Variant::Simple => Err(anyhow!(
                "atom must have an operator or be in format <category>/<package>"
            ))?,
            v => v,
        };

        Ok(Self {
            operator: None,
            category: captures
                .name("category")
                .ok_or_else(|| anyhow!("atom missing <category>"))?
                .as_str()
                .to_owned(),
            package: captures
                .name("package")
                .ok_or_else(|| anyhow!("atom missing <package>"))?
                .as_str()
                .to_owned(),
            version,
            slot: captures
                .name("slot")
                .map(|m| PackageSlot::from_str(m.as_str()))
                .transpose()?,
            repo: captures.name("repo").map(|m| m.as_str().to_owned()),
            variant,
        })
    }

    /// Parses the version including suffixes and revision from the given regex `captures`.
    ///
    /// Return `Ok(None)` if no version can be matched.
    fn parse_version(captures: &Captures) -> Result<Option<PackageVersion>> {
        let version = match captures.name("version") {
            Some(m) => m.as_str(),
            None => return Ok(None),
        };
        let suffixes = captures.name("suffixes").map(|m| m.as_str());
        let revision = captures.name("revision").map(|m| m.as_str());

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
            if self.variant == Variant::VersionWildcard {
                write!(f, "-{version}*")?;
            } else {
                write!(f, "-{version}")?;
            }
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
                    slot: Some(PackageSlot::Simple("1.92.0".into())),
                    ..Default::default()
                },
            ),
            (
                "net-misc/dhcp:*::gentoo",
                Atom {
                    category: "net-misc".into(),
                    package: "dhcp".into(),
                    slot: Some(PackageSlot::Any),
                    repo: Some("gentoo".into()),
                    ..Default::default()
                },
            ),
            (
                "x11-drivers/nvidia-drivers:0/390",
                Atom {
                    category: "x11-drivers".into(),
                    package: "nvidia-drivers".into(),
                    slot: Some(PackageSlot::WithSubSlot("0".into(), "390".into())),
                    ..Default::default()
                },
            ),
        ];

        for (atom_str, expected_atom) in test_cases {
            let atom = Atom::new(atom_str).unwrap();
            assert_eq!(atom, expected_atom);
            assert_eq!(atom.to_string(), atom_str);
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
                    variant: Variant::VersionOperator,
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
                    variant: Variant::VersionOperator,
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
                    variant: Variant::VersionOperator,
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
                    variant: Variant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                ">dev-lang/python-3.14.3_beta-r2:3.14",
                Atom {
                    operator: Some(Operator::Greater),
                    category: "dev-lang".into(),
                    package: "python".into(),
                    version: PackageVersion::new("3.14.3", Some("beta"), Some("2")).ok(),
                    slot: Some(PackageSlot::Simple("3.14".into())),
                    variant: Variant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                "~dev-lang/rust-1.70.0:1.70.0/1=::gentoo",
                Atom {
                    operator: Some(Operator::Approximate),
                    category: "dev-lang".into(),
                    package: "rust".into(),
                    version: PackageVersion::new("1.70.0", None, None).ok(),
                    slot: Some(PackageSlot::EqualsWithSubSlot("1.70.0".into(), "1".into())),
                    repo: Some("gentoo".into()),
                    variant: Variant::VersionOperator,
                    ..Default::default()
                },
            ),
        ];

        for (atom_str, expected_atom) in test_cases {
            let atom = Atom::new(atom_str).unwrap();
            assert_eq!(atom, expected_atom);
            assert_eq!(atom.to_string(), atom_str);
        }
    }

    #[test]
    fn test_atom_from_str_wildcard() {
        let test_cases = vec![
            (
                "=dev-libs/glib-2*",
                Atom {
                    operator: Some(Operator::Equal),
                    category: "dev-libs".into(),
                    package: "glib".into(),
                    version: PackageVersion::new("2", None, None).ok(),
                    variant: Variant::VersionWildcard,
                    ..Default::default()
                },
            ),
            (
                "=dev-lang/rust-1.70*:1.70.0",
                Atom {
                    operator: Some(Operator::Equal),
                    category: "dev-lang".into(),
                    package: "rust".into(),
                    version: PackageVersion::new("1.70", None, None).ok(),
                    variant: Variant::VersionWildcard,
                    slot: Some(PackageSlot::Simple("1.70.0".into())),
                    ..Default::default()
                },
            ),
            (
                "=kde-frameworks/kwindowsystem-6*:6/6.23=::gentoo",
                Atom {
                    operator: Some(Operator::Equal),
                    category: "kde-frameworks".into(),
                    package: "kwindowsystem".into(),
                    version: PackageVersion::new("6", None, None).ok(),
                    variant: Variant::VersionWildcard,
                    slot: Some(PackageSlot::EqualsWithSubSlot("6".into(), "6.23".into())),
                    repo: Some("gentoo".into()),
                    ..Default::default()
                },
            ),
        ];

        for (atom_str, expected_atom) in test_cases {
            let atom = Atom::new(atom_str).unwrap();
            assert_eq!(atom, expected_atom);
            assert_eq!(atom.to_string(), atom_str);
        }
    }

    #[test]
    fn test_atom_from_str_err() {
        let invalid_atoms = vec![
            "invalid-atom",
            "dev-lang/",
            "/rust",
            "x11-drivers/nvidia-drivers:0/",
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
        // TODO: test wildcards and slot matching
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
        let test_data = [
            (
                Atom {
                    category: "dev-lang".into(),
                    package: "python".into(),
                    variant: Variant::Simple,
                    repo: Some("gentoo".into()),
                    ..Default::default()
                },
                "dev-lang/python::gentoo",
            ),
            (
                Atom {
                    operator: Some(Operator::Equal),
                    category: "sys-apps".into(),
                    package: "attr".into(),
                    version: PackageVersion::new("2.5.2", None, Some("1")).ok(),
                    variant: Variant::VersionOperator,
                    ..Default::default()
                },
                "=sys-apps/attr-2.5.2-r1",
            ),
            (
                Atom {
                    operator: Some(Operator::GreaterEqual),
                    category: "dev-lang".into(),
                    package: "rust".into(),
                    version: PackageVersion::new("1.70.0", Some("beta_p11"), Some("2")).ok(),
                    slot: Some(PackageSlot::Simple("1.70".into())),
                    repo: Some("gentoo".into()),
                    variant: Variant::VersionOperator,
                },
                ">=dev-lang/rust-1.70.0_beta_p11-r2:1.70::gentoo",
            ),
            (
                Atom {
                    operator: Some(Operator::Equal),
                    category: "dev-libs".into(),
                    package: "libffi".into(),
                    version: PackageVersion::new("3.5", None, None).ok(),
                    slot: Some(PackageSlot::WithSubSlot("0".into(), "8".into())),
                    variant: Variant::VersionWildcard,
                    ..Default::default()
                },
                "=dev-libs/libffi-3.5*:0/8",
            ),
        ];

        for (atom, expected_str) in test_data {
            assert_eq!(atom.to_string(), expected_str);
        }
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
