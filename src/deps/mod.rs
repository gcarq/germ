use crate::package::Package;
use crate::package::version::PackageVersion;
use anyhow::{Context, Result, anyhow};
use constcat::concat;
use lazy_static::lazy_static;
use regex::{Captures, Regex};
use std::fmt;
use std::str::FromStr;

const OPERATOR: &str = r"(?<operator>[=~]|[><]=?)";

/// PMS 3.1.1 Category names
/// A category name may contain any of the characters [A-Za-z0-9+_.-].
/// It must not begin with a hyphen, a dot or a plus sign.
const CATEGORY: &str = r"(?<category>[\w][\w+.-]*)";

const REPOSITORY: &str = r"[\w][\w-]*";

/// PMS 3.1.2 Package names
/// A package name may contain any of the characters [A-Za-z0-9+_-].
/// It must not begin with a hyphen or a plus sign, and must not end in a hyphen
/// followed by anything matching the version syntax
const PACKAGE: &str = r"(?<package>[\w][\w+-]*?)";
const VERSION: &str =
    r"(?<version>\d+(?:\.\d+)*[a-z]?)(?<suffixes>(?:_(?:alpha|beta|pre|rc|p)\d*)*)";
const REVISION: &str = r"(?<revision>\d*)";
const VR: &str = concat!(VERSION, "(:?-r", REVISION, ")?");

const CATPKG: &str = concat!(CATEGORY, "/", PACKAGE, "(?:-", VR, ")?");
const CATPKGVR: &str = concat!(CATEGORY, "/", PACKAGE, "-", VR);

/// PMS 3.1.3 Slot names
/// A slot name may contain any of the characters [A-Za-z0-9+_.-].
/// It must not begin with a hyphen, a dot or a plus sign.
const SLOT: &str = r"([\w][\w+.-]*)";
const SLOT_LOOSE: &str = r"([\w+./*=-]+)";

lazy_static! {
    /// Regex to capture simple atoms with category and package,
    /// optionally version, slot and repository e.g.: dev-lang/rust, dev-lang/rust-1.70.0.
    static ref ATOM_SIMPLE_RE: Regex = Regex::new(&format!(
        r"^{CATPKG}(?:\:(?P<slot>{SLOT_LOOSE}))?(?:\:\:(?P<repo>{REPOSITORY}))?$"
    ))
    .unwrap();
    /// Regex to capture atoms with operator, category, package,
    /// version, slot and repository e.g.: >=dev-lang/rust-1.70
    static ref ATOM_OPERATOR_RE: Regex = Regex::new(&format!(r"^{OPERATOR}{CATPKGVR}$")).unwrap();
    /// Regex to capture atoms with '=' category, package and a version wildcard.
    static ref ATOM_STAR_RE: Regex = Regex::new(&format!(r"^={CATPKGVR}\*$")).unwrap();
}

#[derive(Debug, Eq, PartialEq, Hash)]
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

#[derive(Debug, Default, Eq, Hash, PartialEq)]
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
#[derive(Debug, PartialEq, Default, Hash, Eq)]
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
                Self::from_regex_capture(&caps, AtomVariant::VersionOperator)
                    .with_context(|| anyhow!("invalid atom: {atom}"))?
                    .with_operator(match caps.name("operator") {
                        Some(m) => Some(Operator::from_str(m.as_str())?),
                        None => None,
                    }),
            );
        }
        if let Some(caps) = ATOM_STAR_RE.captures(atom) {
            return Ok(
                Self::from_regex_capture(&caps, AtomVariant::VersionWildcard)
                    .with_context(|| anyhow!("invalid atom: {atom}"))?
                    .with_operator(Some(Operator::Equal)),
            );
        }
        if let Some(caps) = ATOM_SIMPLE_RE.captures(atom) {
            return Self::from_regex_capture(&caps, AtomVariant::Simple)
                .with_context(|| anyhow!("invalid atom: {atom}"));
        }

        Err(anyhow!("invalid atom: {atom}"))
    }

    /// Returns the qualified name for this atom in the format category/name e.g. app-editors/vim.
    pub fn qualified_name(&self) -> String {
        format!("{}/{}", self.category, self.package)
    }

    /// Checks if the given package matches this atom.
    /// TODO: implement wildcard operator, slot and repo matching
    pub fn matches(&self, pkg: &Package) -> bool {
        if self.category != pkg.category || self.package != pkg.name {
            return false;
        }
        if let Some(version) = &self.version {
            let matches = match self.variant {
                AtomVariant::Simple => version == &pkg.version,
                AtomVariant::VersionOperator => match self.operator {
                    Some(Operator::Less) => pkg.version < *version,
                    Some(Operator::LessEqual) => pkg.version <= *version,
                    Some(Operator::Equal) => {
                        // Equality requires all defined components in the atom to match
                        pkg.version
                            .number
                            .components
                            .iter()
                            .zip(&version.number.components)
                            .all(|(a, b)| a == b)
                            && pkg
                                .version
                                .suffixes
                                .iter()
                                .zip(version.suffixes.iter())
                                .all(|(a, b)| a == b)
                    }
                    Some(Operator::Greater) => pkg.version > *version,
                    Some(Operator::GreaterEqual) => pkg.version >= *version,
                    Some(Operator::Approximate) => {
                        pkg.version.number == version.number
                            && pkg.version.suffixes == version.suffixes
                    }
                    None => unreachable!("BUG: atom is expected to have an operator"),
                },
                AtomVariant::VersionWildcard => todo!("wildcard matching not implemented"),
            };
            if !matches {
                return false;
            }
        }
        true
    }

    /// Creates an Atom from the given regex captures.
    /// It assumes the correct regex has been used.
    /// NOTE: this does not set the operator field, see [`Self::with_operator`].
    fn from_regex_capture(caps: &Captures, variant: AtomVariant) -> Result<Atom> {
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

    fn with_operator(mut self, operator: Option<Operator>) -> Self {
        self.operator = operator;
        self
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
                "=dev-lang/rust-1.70.0",
                Atom {
                    operator: Some(Operator::Equal),
                    category: "dev-lang".into(),
                    package: "rust".into(),
                    version: PackageVersion::new("1.70.0", None, None).ok(),
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
        // TODO: test wildcards and slot/repo matching
        let atoms = vec![
            "sys-devel/gcc",
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
        );
        for atom in atoms {
            let atom = Atom::new(atom).unwrap();
            assert!(atom.matches(&pkg), "{atom} should match {pkg}");
        }
    }

    #[test]
    fn test_atom_matches_false() {
        let atoms = vec![
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
        );
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
