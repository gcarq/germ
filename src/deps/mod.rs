mod version;

use crate::package::{PackageVersion, PackageVersionSuffix};
use anyhow::{Result, anyhow};
use constcat::concat;
use lazy_static::lazy_static;
use regex::{Captures, Regex};
use std::fmt;

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

impl Operator {
    pub fn from_str(op_str: &str) -> Option<Self> {
        match op_str {
            "<" => Some(Operator::Less),
            "<=" => Some(Operator::LessEqual),
            "=" => Some(Operator::Equal),
            ">" => Some(Operator::Greater),
            ">=" => Some(Operator::GreaterEqual),
            "~" => Some(Operator::Approximate),
            _ => None,
        }
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

/// Represents a portage package atom. An atom is simply a dependency that is used by portage when
/// calculating relationships between packages.
/// TODO:
///  * implement remaining atom variants (see man 5 ebuild)
///  * implement version comparison logic
///  * implement matching logic between atoms and packages
#[derive(Debug, PartialEq, Default, Hash, Eq)]
pub struct Atom {
    operator: Option<Operator>,
    category: String,
    package: String,
    version: Option<PackageVersion>,
    slot: Option<String>,
    repo: Option<String>,
}

impl Atom {
    /// Creates an Atom from the given string representation.
    /// Returns an error if the string is not a valid atom.
    pub fn new(atom: &str) -> Result<Self> {
        if let Some(caps) = ATOM_OPERATOR_RE.captures(atom) {
            return Ok(Self::from_regex_capture(&caps)?.with_operator(
                match caps.name("operator") {
                    Some(m) => Operator::from_str(m.as_str()),
                    None => None,
                },
            ));
        }
        if let Some(caps) = ATOM_STAR_RE.captures(atom) {
            return Ok(Self::from_regex_capture(&caps)?.with_operator(Some(Operator::Equal)));
        }
        if let Some(caps) = ATOM_SIMPLE_RE.captures(atom) {
            return Self::from_regex_capture(&caps);
        }

        Err(anyhow!("invalid atom: {atom}"))
    }

    /// Creates an Atom from the given regex captures.
    /// It assumes the correct regex has been used.
    /// NOTE: this does not set the operator field, see [`Self::with_operator`].
    fn from_regex_capture(caps: &Captures) -> Result<Atom> {
        Ok(Self {
            operator: None,
            category: match caps.name("category") {
                Some(m) => m.as_str(),
                None => "*",
            }
            .to_string(),
            package: match caps.name("package") {
                Some(m) => m.as_str(),
                None => "*",
            }
            .to_string(),
            version: Self::parse_version_components(caps),
            slot: caps.name("slot").map(|m| m.as_str().to_string()),
            repo: caps.name("repo").map(|m| m.as_str().to_string()),
        })
    }

    /// Parses version components from the given regex captures if present,
    /// that includes version, suffixes and revision.
    fn parse_version_components(caps: &Captures) -> Option<PackageVersion> {
        let version = match caps.name("version") {
            Some(m) => m.as_str(),
            None => return None,
        };
        let suffixes = caps
            .name("suffixes")
            .map(|s| {
                s.as_str()
                    .split('_')
                    .filter(|s| !s.is_empty())
                    .map(PackageVersionSuffix::new)
                    .collect::<Vec<PackageVersionSuffix>>()
            })
            .unwrap_or_default();
        Some(PackageVersion::new(
            version.to_string(),
            suffixes,
            caps.name("revision")
                .and_then(|r| r.as_str().parse::<usize>().ok())
                .unwrap_or(0),
        ))
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
            write!(f, "-{}", version)?;
        }
        if let Some(slot) = &self.slot {
            write!(f, ":{}", slot)?;
        }
        if let Some(repo) = &self.repo {
            write!(f, "::{}", repo)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operator_from_str() {
        assert_eq!(Operator::from_str("<"), Some(Operator::Less));
        assert_eq!(Operator::from_str("<="), Some(Operator::LessEqual));
        assert_eq!(Operator::from_str("="), Some(Operator::Equal));
        assert_eq!(Operator::from_str(">"), Some(Operator::Greater));
        assert_eq!(Operator::from_str(">="), Some(Operator::GreaterEqual));
        assert_eq!(Operator::from_str("~"), Some(Operator::Approximate));
        assert_eq!(Operator::from_str("invalid"), None);
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
                "sys-apps/sed-4.0.5",
                Atom {
                    category: "sys-apps".into(),
                    package: "sed".into(),
                    version: Some(PackageVersion::new("4.0.5".into(), Vec::new(), 0)),
                    ..Default::default()
                },
            ),
            (
                "sys-libs/zlib-1.1.4-r1",
                Atom {
                    category: "sys-libs".into(),
                    package: "zlib".into(),
                    version: Some(PackageVersion::new("1.1.4".into(), Vec::new(), 1)),
                    ..Default::default()
                },
            ),
            (
                "net-misc/dhcp-3.0_p2",
                Atom {
                    category: "net-misc".into(),
                    package: "dhcp".into(),
                    version: Some(PackageVersion::new(
                        "3.0".into(),
                        vec![PackageVersionSuffix::new("p2")],
                        0,
                    )),
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
                "media-libs/mesa-9999::x11",
                Atom {
                    category: "media-libs".into(),
                    package: "mesa".into(),
                    version: Some(PackageVersion::new("9999".into(), Vec::new(), 0)),
                    repo: Some("x11".into()),
                    ..Default::default()
                },
            ),
            (
                "net-misc/dhcp-3.0_p2:0::gentoo",
                Atom {
                    category: "net-misc".into(),
                    package: "dhcp".into(),
                    version: Some(PackageVersion::new(
                        "3.0".into(),
                        vec![PackageVersionSuffix::new("p2")],
                        0,
                    )),
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
            let atom = Atom::new(atom_str).unwrap();
            assert_eq!(atom, expected_atom);
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
                    version: Some(PackageVersion::new("1.70.0".into(), Vec::new(), 0)),
                    ..Default::default()
                },
            ),
            (
                ">=sys-apps/sed-4.8",
                Atom {
                    operator: Some(Operator::GreaterEqual),
                    category: "sys-apps".into(),
                    package: "sed".into(),
                    version: Some(PackageVersion::new("4.8".into(), Vec::new(), 0)),
                    ..Default::default()
                },
            ),
            (
                "<=net-misc/dhcp-3.0_p2",
                Atom {
                    operator: Some(Operator::LessEqual),
                    category: "net-misc".into(),
                    package: "dhcp".into(),
                    version: Some(PackageVersion::new(
                        "3.0".into(),
                        vec![PackageVersionSuffix::new("p2")],
                        0,
                    )),
                    ..Default::default()
                },
            ),
            (
                "<net-misc/dhcp-3",
                Atom {
                    operator: Some(Operator::Less),
                    category: "net-misc".into(),
                    package: "dhcp".into(),
                    version: Some(PackageVersion::new("3".into(), Vec::new(), 0)),
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
                    version: Some(PackageVersion::new("1.70.0".into(), Vec::new(), 0)),
                    ..Default::default()
                },
            ),
            (
                "=dev-libs/glib-2*",
                Atom {
                    operator: Some(Operator::Equal),
                    category: "dev-libs".into(),
                    package: "glib".into(),
                    version: Some(PackageVersion::new("2".into(), Vec::new(), 0)),
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
    fn test_atom_from_str_invalid() {
        let invalid_atoms = vec![
            "invalid-atom",
            "dev-lang/",
            "/rust",
            ">=dev-lang/rust-",
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
    fn test_atom_display() {
        let atom = Atom {
            operator: Some(Operator::GreaterEqual),
            category: "dev-lang".into(),
            package: "rust".into(),
            version: Some(PackageVersion::new(
                "1.70.0".into(),
                vec![
                    PackageVersionSuffix::new("beta"),
                    PackageVersionSuffix::new("p11"),
                ],
                2,
            )),
            slot: Some("1.70".into()),
            repo: Some("gentoo".into()),
        };
        let atom_str = atom.to_string();
        assert_eq!(atom_str, ">=dev-lang/rust-1.70.0_beta_p11-r2:1.70::gentoo");
    }
}
