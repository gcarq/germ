use crate::deps::UseFlag;
use crate::deps::expr::ExpressionItem;
use crate::package::slot::PackageSlot;
use crate::package::version::PackageVersion;
use crate::regex::{CATEGORY, PACKAGE, PV_REV, REPOSITORY, V_REV};
use anyhow::{Result, anyhow};
use constcat::concat;
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::Write;
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
/// Captures optional use flag in atoms, e.g. `=dev-lang/rust-1.70.0[clippy]`.
const ATOM_USEDEP_LOOSE: &str = r"(\[(?P<use_deps>.*)\])?";
/// Regex to capture simple atoms with category and package,
/// optionally version, slot and repository e.g.: `dev-lang/rust-1.70.0`.
static ATOM_SIMPLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^{ATOM_CP}{ATOM_SLOT_LOOSE}{ATOM_REPOSITORY}{ATOM_USEDEP_LOOSE}$"
    ))
    .unwrap()
});
/// Regex to capture atoms with operator, category, package,
/// version, ... e.g.: `>=dev-lang/rust-1.70`
static ATOM_OPERATOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^{ATOM_OPERATOR}{ATOM_CPV_REV}{ATOM_SLOT_LOOSE}{ATOM_REPOSITORY}{ATOM_USEDEP_LOOSE}$"
    ))
    .unwrap()
});
/// Regex to capture atoms with an equal operator, category, package and a version wildcard, ...
/// e.g.: `=dev-lang/rust-1.70*`
static ATOM_WILDCARD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^={ATOM_CPV_REV}\*{ATOM_SLOT_LOOSE}{ATOM_REPOSITORY}{ATOM_USEDEP_LOOSE}$"
    ))
    .unwrap()
});

/// Specifies the operator of an atom, which determines how packages are matched.
#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[cfg_attr(test, derive(Debug))]
pub enum AtomOperator {
    Less,
    LessEqual,
    Equal,
    Greater,
    GreaterEqual,
    Approximate,
}

impl FromStr for AtomOperator {
    type Err = anyhow::Error;

    fn from_str(operator: &str) -> Result<Self> {
        let op = match operator {
            "<" => AtomOperator::Less,
            "<=" => AtomOperator::LessEqual,
            "=" => AtomOperator::Equal,
            ">" => AtomOperator::Greater,
            ">=" => AtomOperator::GreaterEqual,
            "~" => AtomOperator::Approximate,
            _ => return Err(anyhow!("invalid operator: {operator}")),
        };
        Ok(op)
    }
}

impl fmt::Display for AtomOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let op = match self {
            AtomOperator::Less => "<",
            AtomOperator::LessEqual => "<=",
            AtomOperator::Equal => "=",
            AtomOperator::Greater => ">",
            AtomOperator::GreaterEqual => ">=",
            AtomOperator::Approximate => "~",
        };
        f.write_str(op)
    }
}

/// Specifies the variant of an atom, which determines how packages are matched.
#[derive(Serialize, Deserialize, Default, Clone, Eq, Hash, PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub enum AtomVariant {
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
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[cfg_attr(test, derive(Default, Debug))]
pub struct Atom {
    pub operator: Option<AtomOperator>,
    pub category: String,
    pub package: String,
    pub version: Option<PackageVersion>,
    pub slot: Option<PackageSlot>,
    pub repo: Option<String>,
    pub use_deps: Option<Vec<UseFlag>>,
    pub variant: AtomVariant,
}

impl Atom {
    /// Creates an [`Atom`] from the given `atom` string.
    ///
    /// Returns `Err` if the string is not a valid atom.
    pub fn new(atom: &str) -> Result<Self> {
        if let Some(caps) = ATOM_OPERATOR_RE.captures(atom) {
            return Ok(
                Self::from_regex_capture(&caps, AtomVariant::VersionOperator)?.with_operator(
                    match caps.name("operator") {
                        Some(m) => Some(AtomOperator::from_str(m.as_str())?),
                        None => None,
                    },
                ),
            );
        }
        if let Some(caps) = ATOM_WILDCARD_RE.captures(atom) {
            return Ok(
                Self::from_regex_capture(&caps, AtomVariant::VersionWildcard)?
                    .with_operator(Some(AtomOperator::Equal)),
            );
        }
        if let Some(caps) = ATOM_SIMPLE_RE.captures(atom) {
            return Self::from_regex_capture(&caps, AtomVariant::Simple);
        }

        Err(anyhow!("'{atom}' is not a valid package atom"))
    }

    /// Returns the qualified name for this atom in the format
    /// `category/name` e.g. `app-editors/vim`.
    pub fn qualified_name(&self) -> String {
        format!("{}/{}", self.category, self.package)
    }

    /// Creates an Atom from the given regex `captures` and `variant`.
    ///
    /// It assumes the correct regex has been used.
    /// NOTE: this does not set the operator field, see [`Self::with_operator`].
    fn from_regex_capture(captures: &Captures, variant: AtomVariant) -> Result<Self> {
        let version = match Self::parse_version(captures)? {
            Some(_) if variant == AtomVariant::Simple => Err(anyhow!(
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
            use_deps: captures
                .name("use_deps")
                .map(|m| {
                    m.as_str()
                        .split(',')
                        .map(UseFlag::parse)
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?,
            variant,
        })
    }

    const fn with_operator(mut self, operator: Option<AtomOperator>) -> Self {
        self.operator = operator;
        self
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
}

impl ExpressionItem for Atom {}

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
        f.write_str(&self.category)?;
        f.write_char('/')?;
        f.write_str(&self.package)?;
        if let Some(version) = &self.version {
            f.write_char('-')?;
            if self.variant == AtomVariant::VersionWildcard {
                write!(f, "{version}")?;
                f.write_char('*')?;
            } else {
                write!(f, "{version}")?;
            }
        }
        if let Some(slot) = &self.slot {
            f.write_char(':')?;
            write!(f, "{slot}")?;
        }
        if let Some(repo) = &self.repo {
            f.write_str("::")?;
            f.write_str(repo)?;
        }
        if let Some(use_deps) = &self.use_deps {
            f.write_char('[')?;
            for (i, use_dep) in use_deps.iter().enumerate() {
                if i > 0 {
                    f.write_str(",")?;
                }
                write!(f, "{use_dep}")?;
            }
            f.write_char(']')?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::deps::atom::{Atom, AtomOperator, AtomVariant};
    use crate::deps::*;
    use crate::package::slot::PackageSlot;
    use crate::package::version::PackageVersion;

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
                    slot: Some(PackageSlot::Eq("1.92.0".into())),
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
                    slot: Some(PackageSlot::EqSubSlot("0".into(), "390".into())),
                    ..Default::default()
                },
            ),
            (
                "sys-libs/glibc[audit,caps(-)]",
                Atom {
                    category: "sys-libs".into(),
                    package: "glibc".into(),
                    use_deps: Some(vec![UseFlag("audit".into()), UseFlag("caps(-)".into())]),
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
                    operator: Some(AtomOperator::Equal),
                    category: "sys-apps".into(),
                    package: "memtest86+".into(),
                    version: PackageVersion::new("7.2.0", None, None).ok(),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                ">=sys-apps/sed-4.8",
                Atom {
                    operator: Some(AtomOperator::GreaterEqual),
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
                    operator: Some(AtomOperator::Less),
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
                    operator: Some(AtomOperator::LessEqual),
                    category: "net-misc".into(),
                    package: "dhcp".into(),
                    version: PackageVersion::new("3.0", Some("p2"), None).ok(),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                ">dev-lang/python-3.14.3_beta-r2:3.14",
                Atom {
                    operator: Some(AtomOperator::Greater),
                    category: "dev-lang".into(),
                    package: "python".into(),
                    version: PackageVersion::new("3.14.3", Some("beta"), Some("2")).ok(),
                    slot: Some(PackageSlot::Eq("3.14".into())),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                "~dev-lang/rust-1.70.0:1.70.0/1::gentoo",
                Atom {
                    operator: Some(AtomOperator::Approximate),
                    category: "dev-lang".into(),
                    package: "rust".into(),
                    version: PackageVersion::new("1.70.0", None, None).ok(),
                    slot: Some(PackageSlot::EqSubSlot("1.70.0".into(), "1".into())),
                    repo: Some("gentoo".into()),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                ">=sys-libs/glibc-2.41-r10:2.2::gentoo[cet,clang]",
                Atom {
                    operator: Some(AtomOperator::GreaterEqual),
                    category: "sys-libs".into(),
                    package: "glibc".into(),
                    version: PackageVersion::new("2.41", None, Some("10")).ok(),
                    slot: Some(PackageSlot::Eq("2.2".into())),
                    repo: Some("gentoo".into()),
                    variant: AtomVariant::VersionOperator,
                    use_deps: Some(vec![UseFlag("cet".into()), UseFlag("clang".into())]),
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
                    operator: Some(AtomOperator::Equal),
                    category: "dev-libs".into(),
                    package: "glib".into(),
                    version: PackageVersion::new("2", None, None).ok(),
                    variant: AtomVariant::VersionWildcard,
                    ..Default::default()
                },
            ),
            (
                "=dev-lang/rust-1.70*:1.70.0",
                Atom {
                    operator: Some(AtomOperator::Equal),
                    category: "dev-lang".into(),
                    package: "rust".into(),
                    version: PackageVersion::new("1.70", None, None).ok(),
                    variant: AtomVariant::VersionWildcard,
                    slot: Some(PackageSlot::Eq("1.70.0".into())),
                    ..Default::default()
                },
            ),
            (
                "=kde-frameworks/kwindowsystem-6*:6/6.23::gentoo",
                Atom {
                    operator: Some(AtomOperator::Equal),
                    category: "kde-frameworks".into(),
                    package: "kwindowsystem".into(),
                    version: PackageVersion::new("6", None, None).ok(),
                    variant: AtomVariant::VersionWildcard,
                    slot: Some(PackageSlot::EqSubSlot("6".into(), "6.23".into())),
                    repo: Some("gentoo".into()),
                    ..Default::default()
                },
            ),
            (
                "=app-arch/7zip-26*[rar]",
                Atom {
                    operator: Some(AtomOperator::Equal),
                    category: "app-arch".into(),
                    package: "7zip".into(),
                    version: PackageVersion::new("26", None, None).ok(),
                    variant: AtomVariant::VersionWildcard,
                    use_deps: Some(vec![UseFlag("rar".into())]),
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
            "dev-lang/rust[]",
            "dev-lang/rust[,]",
        ];

        for atom_str in invalid_atoms {
            assert!(Atom::new(atom_str).is_err(), "{atom_str} should be invalid");
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
                    variant: AtomVariant::Simple,
                    repo: Some("gentoo".into()),
                    ..Default::default()
                },
                "dev-lang/python::gentoo",
            ),
            (
                Atom {
                    operator: Some(AtomOperator::Equal),
                    category: "sys-apps".into(),
                    package: "attr".into(),
                    version: PackageVersion::new("2.5.2", None, Some("1")).ok(),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
                "=sys-apps/attr-2.5.2-r1",
            ),
            (
                Atom {
                    operator: Some(AtomOperator::GreaterEqual),
                    category: "dev-lang".into(),
                    package: "rust".into(),
                    version: PackageVersion::new("1.70.0", Some("beta_p11"), Some("2")).ok(),
                    slot: Some(PackageSlot::Eq("1.70".into())),
                    repo: Some("gentoo".into()),
                    variant: AtomVariant::VersionOperator,
                    use_deps: Some(vec![UseFlag("clippy".into())]),
                    ..Default::default()
                },
                ">=dev-lang/rust-1.70.0_beta_p11-r2:1.70::gentoo[clippy]",
            ),
            (
                Atom {
                    operator: Some(AtomOperator::Equal),
                    category: "dev-libs".into(),
                    package: "libffi".into(),
                    version: PackageVersion::new("3.5", None, None).ok(),
                    slot: Some(PackageSlot::EqSubSlot("0".into(), "8".into())),
                    variant: AtomVariant::VersionWildcard,
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
            ("<", AtomOperator::Less),
            ("<=", AtomOperator::LessEqual),
            ("=", AtomOperator::Equal),
            (">", AtomOperator::Greater),
            (">=", AtomOperator::GreaterEqual),
            ("~", AtomOperator::Approximate),
        ];
        for (op_str, expected_op) in test_cases {
            let op = AtomOperator::from_str(op_str).unwrap();
            assert_eq!(op, expected_op);
        }
    }

    #[test]
    fn test_operator_from_str_err() {
        let invalid_ops = vec!["!", "==", "><", "=>", "invalid"];
        for op_str in invalid_ops {
            assert!(
                AtomOperator::from_str(op_str).is_err(),
                "{op_str} should be invalid"
            );
        }
    }

    #[test]
    fn test_operator_display() {
        assert_eq!(AtomOperator::Less.to_string(), "<");
        assert_eq!(AtomOperator::LessEqual.to_string(), "<=");
        assert_eq!(AtomOperator::Equal.to_string(), "=");
        assert_eq!(AtomOperator::Greater.to_string(), ">");
        assert_eq!(AtomOperator::GreaterEqual.to_string(), ">=");
        assert_eq!(AtomOperator::Approximate.to_string(), "~");
    }
}
