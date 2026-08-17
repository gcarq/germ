use crate::useflag::UseDep;

use crate::grammar::{CATEGORY, PACKAGE, REPOSITORY, REVISION, VERSION, VERSION_SUFFIXES};
use crate::package::names::{CatName, PkgName};
use crate::package::slot::PackageSlot;
use crate::package::version::PackageVersion;
use crate::repository::RepoName;
use anyhow::{Context, anyhow, bail};
use fancy_regex::{Captures, Regex};
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt::{self, Write};
use std::str::FromStr;
use std::sync::LazyLock;

static ATOM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?x)
        \A
        (?:
            (?<operator>[=~]|[><]=?)
            (?<operator_category>{CATEGORY}) /
            (?<operator_package>{PACKAGE}) -
            (?<operator_version>{VERSION})
            (?<operator_suffixes>{VERSION_SUFFIXES})
            (?: -r (?<operator_revision>{REVISION}) )?

          |

            =
            (?<wildcard_category>{CATEGORY}) /
            (?<wildcard_package>{PACKAGE}) -
            (?<wildcard_version>{VERSION})
            (?<wildcard_suffixes>{VERSION_SUFFIXES})
            (?: -r (?<wildcard_revision>{REVISION}) )?
            \*

          |

            (?<simple_category>{CATEGORY}|\*) /
            (?<simple_package>{PACKAGE}|\*)
        )
        (?: : (?<slot>[a-zA-Z0-9_+./*=-]+) )?
        (?: :: (?<repo>{REPOSITORY}) )?
        (?: \[ (?<use_deps>.*) \] )?
        \z"
    ))
    .unwrap()
});

/// Specifies the operator of an atom, which determines how packages are matched.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum AtomOperator {
    Less,
    LessEqual,
    Equal,
    Greater,
    GreaterEqual,
    Approximate,
}

impl AtomOperator {
    pub fn new(operator: &str) -> anyhow::Result<Self> {
        let op = match operator {
            "<" => AtomOperator::Less,
            "<=" => AtomOperator::LessEqual,
            "=" => AtomOperator::Equal,
            ">" => AtomOperator::Greater,
            ">=" => AtomOperator::GreaterEqual,
            "~" => AtomOperator::Approximate,
            _ => bail!("invalid operator: {operator}"),
        };
        Ok(op)
    }
}

impl FromStr for AtomOperator {
    type Err = anyhow::Error;

    fn from_str(operator: &str) -> anyhow::Result<Self> {
        Self::new(operator)
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
#[derive(
    Archive, Serialize, Deserialize, Default, Clone, PartialEq, Eq, Ord, PartialOrd, Hash, Debug,
)]
pub enum AtomVariant {
    #[default]
    Simple,
    VersionOperator,
    VersionWildcard,
}

/// This enum defines two variants to express identifier matching for e.g. `category` and `package`.
///
/// [`Self::Exact`] should only match an exact name and [`Self::Any`] should match all values.
#[derive(
    Archive, Serialize, Deserialize, Default, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug,
)]
pub enum AtomIdent<T> {
    Exact(T),
    #[default]
    Any,
}

impl<T> FromStr for AtomIdent<T>
where
    T: FromStr<Err = anyhow::Error>,
{
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "*" => Ok(Self::Any),
            _ => Ok(Self::Exact(value.parse()?)),
        }
    }
}

impl<T: PartialEq> AtomIdent<T> {
    pub fn matches(&self, value: &T) -> bool {
        match self {
            Self::Exact(expected) => expected == value,
            Self::Any => true,
        }
    }
}

impl<T: fmt::Display> fmt::Display for AtomIdent<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AtomIdent::Exact(inner) => write!(f, "{inner}"),
            AtomIdent::Any => f.write_char('*'),
        }
    }
}

/// Represents a portage package atom.
///
/// An atom can match one or more [`Package`] and is used for
/// calculating dependencies between packages.
/// TODO:
///  * implement remaining atom variants (see man 5 ebuild)
#[derive(Archive, Serialize, Deserialize, Clone, PartialEq, Eq, Ord, PartialOrd, Hash, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct Atom {
    pub operator: Option<AtomOperator>,
    pub category: AtomIdent<CatName>,
    pub package: AtomIdent<PkgName>,
    pub version: Option<PackageVersion>,
    pub slot: Option<PackageSlot>,
    pub repo: Option<RepoName>,
    pub use_deps: Vec<UseDep>,
    pub variant: AtomVariant,
}

impl Atom {
    /// Creates an [`Atom`] from the given `atom` string.
    ///
    /// Returns `Err` if the string is not a valid atom.
    pub fn new(atom: &str) -> anyhow::Result<Self> {
        let Some(captures) = ATOM_RE.captures(atom)? else {
            bail!("'{atom}' is not a valid atom");
        };

        Self::from_regex_capture(&captures)
            .with_context(|| anyhow!("unable to parse atom '{atom}'"))
    }

    /// Returns the qualified name for this atom in the format
    /// `category/name` e.g. `app-editors/vim`.
    pub fn qualified_name(&self) -> String {
        format!("{}/{}", self.category, self.package)
    }

    /// Creates an [`Atom`] from the captures of the atom grammar.
    fn from_regex_capture(captures: &Captures<'_, str>) -> anyhow::Result<Self> {
        let variant = if captures.name("operator").is_some() {
            AtomVariant::VersionOperator
        } else if captures.name("wildcard_version").is_some() {
            AtomVariant::VersionWildcard
        } else {
            AtomVariant::Simple
        };

        Ok(Self {
            operator: captures
                .name("operator")
                .map(|capture| capture.as_str().parse())
                .transpose()?
                .or_else(|| {
                    (variant == AtomVariant::VersionWildcard).then_some(AtomOperator::Equal)
                }),
            category: Self::first_capture(
                captures,
                ["operator_category", "wildcard_category", "simple_category"],
            )
            .ok_or_else(|| anyhow!("atom missing <category>"))?
            .parse()?,
            package: Self::first_capture(
                captures,
                ["operator_package", "wildcard_package", "simple_package"],
            )
            .ok_or_else(|| anyhow!("atom missing <package>"))?
            .parse()?,
            version: Self::parse_version(captures)?,
            slot: captures
                .name("slot")
                .map(|capture| capture.as_str().parse())
                .transpose()?,
            repo: captures
                .name("repo")
                .map(|capture| capture.as_str().parse())
                .transpose()?,
            use_deps: captures
                .name("use_deps")
                .map(|capture| {
                    capture
                        .as_str()
                        .split(',')
                        .map(str::parse::<UseDep>)
                        .collect::<anyhow::Result<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default(),
            variant,
        })
    }

    /// Parses the version based on the selected atom.
    fn parse_version(captures: &Captures<'_, str>) -> anyhow::Result<Option<PackageVersion>> {
        let Some(version) = Self::first_capture(captures, ["operator_version", "wildcard_version"])
        else {
            return Ok(None);
        };

        Ok(Some(PackageVersion::new(
            version,
            Self::first_capture(captures, ["operator_suffixes", "wildcard_suffixes"]),
            Self::first_capture(captures, ["operator_revision", "wildcard_revision"]),
        )?))
    }

    /// Returns the first capture from the given `names` that is present in `captures`.
    fn first_capture<'a>(
        captures: &'a Captures<'_, str>,
        names: impl IntoIterator<Item = &'a str>,
    ) -> Option<&'a str> {
        names
            .into_iter()
            .find_map(|name| captures.name(name))
            .map(|cap| cap.as_str())
    }
}

impl FromStr for Atom {
    type Err = anyhow::Error;

    fn from_str(atom: &str) -> anyhow::Result<Self> {
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
            if self.variant == AtomVariant::VersionWildcard {
                f.write_char('*')?;
            }
        }
        if let Some(slot) = &self.slot {
            f.write_char(':')?;
            write!(f, "{slot}")?;
        }
        if let Some(repo) = &self.repo {
            f.write_str("::")?;
            f.write_str(repo.as_str())?;
        }
        if !self.use_deps.is_empty() {
            f.write_char('[')?;
            for (i, use_dep) in self.use_deps.iter().enumerate() {
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
    use super::AtomIdent::{Any, Exact};
    use super::*;

    impl From<&str> for CatName {
        fn from(value: &str) -> Self {
            value.parse().unwrap()
        }
    }

    impl From<&str> for PkgName {
        fn from(value: &str) -> Self {
            value.parse().unwrap()
        }
    }

    #[test]
    fn test_atom_from_str_simple() {
        let test_cases = vec![
            (
                "dev-lang/rust",
                Atom {
                    category: Exact("dev-lang".into()),
                    package: Exact("rust".into()),
                    ..Default::default()
                },
            ),
            (
                "*/*",
                Atom {
                    category: Any,
                    package: Any,
                    ..Default::default()
                },
            ),
            (
                "*/rust",
                Atom {
                    category: Any,
                    package: Exact("rust".into()),
                    ..Default::default()
                },
            ),
            (
                "dev-lang/*",
                Atom {
                    category: Exact("dev-lang".into()),
                    package: Any,
                    ..Default::default()
                },
            ),
            (
                "dev-lang/rust:1.92.0",
                Atom {
                    category: Exact("dev-lang".into()),
                    package: Exact("rust".into()),
                    slot: Some(PackageSlot::Eq("1.92.0".into())),
                    ..Default::default()
                },
            ),
            (
                "cat/foo-r2",
                Atom {
                    category: Exact("cat".into()),
                    package: Exact("foo-r2".into()),
                    ..Default::default()
                },
            ),
            (
                "cat/foo-::repo-",
                Atom {
                    category: Exact("cat".into()),
                    package: Exact("foo-".into()),
                    repo: Some("repo-".parse().unwrap()),
                    ..Default::default()
                },
            ),
            (
                "net-misc/*:*::gentoo",
                Atom {
                    category: Exact("net-misc".into()),
                    package: Any,
                    slot: Some(PackageSlot::Any),
                    repo: Some("gentoo".parse().unwrap()),
                    ..Default::default()
                },
            ),
            (
                "net-misc/dhcp:*::gentoo",
                Atom {
                    category: Exact("net-misc".into()),
                    package: Exact("dhcp".into()),
                    slot: Some(PackageSlot::Any),
                    repo: Some("gentoo".parse().unwrap()),
                    ..Default::default()
                },
            ),
            (
                "x11-drivers/nvidia-drivers:0/390",
                Atom {
                    category: Exact("x11-drivers".into()),
                    package: Exact("nvidia-drivers".into()),
                    slot: Some(PackageSlot::EqSubSlot("0".into(), "390".into())),
                    ..Default::default()
                },
            ),
            (
                "sys-libs/glibc[audit,caps(-)]",
                Atom {
                    category: Exact("sys-libs".into()),
                    package: Exact("glibc".into()),
                    use_deps: vec!["audit".parse().unwrap(), "caps(-)".parse().unwrap()],
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
    #[allow(clippy::too_many_lines)]
    fn test_atom_from_str_operator() {
        let test_cases = vec![
            (
                "=sys-apps/memtest86+-7.2.0",
                Atom {
                    operator: Some(AtomOperator::Equal),
                    category: Exact("sys-apps".into()),
                    package: Exact("memtest86+".into()),
                    version: PackageVersion::try_from("7.2.0").ok(),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                "=cat/pkg-1-r2",
                Atom {
                    operator: Some(AtomOperator::Equal),
                    category: Exact("cat".into()),
                    package: Exact("pkg".into()),
                    version: PackageVersion::try_from("1-r2").ok(),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                "=cat/foo-r2-2",
                Atom {
                    operator: Some(AtomOperator::Equal),
                    category: Exact("cat".into()),
                    package: Exact("foo-r2".into()),
                    version: PackageVersion::try_from("2").ok(),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                ">=sys-apps/sed-4.8",
                Atom {
                    operator: Some(AtomOperator::GreaterEqual),
                    category: Exact("sys-apps".into()),
                    package: Exact("sed".into()),
                    version: PackageVersion::try_from("4.8").ok(),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                "<net-misc/dhcp-3",
                Atom {
                    operator: Some(AtomOperator::Less),
                    category: Exact("net-misc".into()),
                    package: Exact("dhcp".into()),
                    version: PackageVersion::try_from("3").ok(),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                "<=net-misc/dhcp-3.0_p2",
                Atom {
                    operator: Some(AtomOperator::LessEqual),
                    category: Exact("net-misc".into()),
                    package: Exact("dhcp".into()),
                    version: PackageVersion::try_from("3.0_p2").ok(),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                ">dev-lang/python-3.14.3_beta-r2:3.14",
                Atom {
                    operator: Some(AtomOperator::Greater),
                    category: Exact("dev-lang".into()),
                    package: Exact("python".into()),
                    version: PackageVersion::try_from("3.14.3_beta-r2").ok(),
                    slot: Some(PackageSlot::Eq("3.14".into())),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                "~dev-lang/rust-1.70.0:1.70.0/1::gentoo",
                Atom {
                    operator: Some(AtomOperator::Approximate),
                    category: Exact("dev-lang".into()),
                    package: Exact("rust".into()),
                    version: PackageVersion::try_from("1.70.0").ok(),
                    slot: Some(PackageSlot::EqSubSlot("1.70.0".into(), "1".into())),
                    repo: Some("gentoo".parse().unwrap()),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
            ),
            (
                ">=sys-libs/glibc-2.41-r10:2.2::gentoo[cet,clang]",
                Atom {
                    operator: Some(AtomOperator::GreaterEqual),
                    category: Exact("sys-libs".into()),
                    package: Exact("glibc".into()),
                    version: PackageVersion::try_from("2.41-r10").ok(),
                    slot: Some(PackageSlot::Eq("2.2".into())),
                    repo: Some("gentoo".parse().unwrap()),
                    variant: AtomVariant::VersionOperator,
                    use_deps: vec!["cet".parse().unwrap(), "clang".parse().unwrap()],
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
                    category: Exact("dev-libs".into()),
                    package: Exact("glib".into()),
                    version: PackageVersion::try_from("2").ok(),
                    variant: AtomVariant::VersionWildcard,
                    ..Default::default()
                },
            ),
            (
                "=dev-lang/rust-1.70*:1.70.0",
                Atom {
                    operator: Some(AtomOperator::Equal),
                    category: Exact("dev-lang".into()),
                    package: Exact("rust".into()),
                    version: PackageVersion::try_from("1.70").ok(),
                    variant: AtomVariant::VersionWildcard,
                    slot: Some(PackageSlot::Eq("1.70.0".into())),
                    ..Default::default()
                },
            ),
            (
                "=kde-frameworks/kwindowsystem-6*:6/6.23::gentoo",
                Atom {
                    operator: Some(AtomOperator::Equal),
                    category: Exact("kde-frameworks".into()),
                    package: Exact("kwindowsystem".into()),
                    version: PackageVersion::try_from("6").ok(),
                    variant: AtomVariant::VersionWildcard,
                    slot: Some(PackageSlot::EqSubSlot("6".into(), "6.23".into())),
                    repo: Some("gentoo".parse().unwrap()),
                    ..Default::default()
                },
            ),
            (
                "=app-arch/7zip-26*[rar]",
                Atom {
                    operator: Some(AtomOperator::Equal),
                    category: Exact("app-arch".into()),
                    package: Exact("7zip".into()),
                    version: PackageVersion::try_from("26").ok(),
                    variant: AtomVariant::VersionWildcard,
                    use_deps: vec!["rar".parse().unwrap()],
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
    fn test_atom_use_deps() {
        let atom = Atom::new("cat/pkg[foo,-bar,!baz?,qux(-)=]").unwrap();
        assert_eq!(
            atom.use_deps
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["foo", "-bar", "!baz?", "qux(-)="]
        );
        assert_eq!(atom.to_string(), "cat/pkg[foo,-bar,!baz?,qux(-)=]");
        assert!(Atom::new("cat/pkg").unwrap().use_deps.is_empty());

        for atom in [
            "cat/pkg[]",
            "cat/pkg[foo,]",
            "cat/pkg[foo bar]",
            "cat/pkg[foo, -bar]",
            "cat/pkg[!foo]",
            "cat/pkg[-foo?]",
        ] {
            assert!(Atom::new(atom).is_err(), "{atom} should be invalid");
        }
    }

    #[test]
    fn test_atom_from_str_err() {
        let invalid_atoms = vec![
            "invalid-atom",
            "dev-lang/",
            "/rust",
            "x11-drivers/nvidia-drivers:0/",
            "<=net-misc/*-3.0_p2",
            ">=dev-lang/rust-",
            "dev-lang/rust-1.70.0",
            "cat/pkg-1",
            "cat/pkg-1-2",
            "cat/pkg-1-r2",
            "cat/pkg::repo-1",
            "=cat/pkg-1-2",
            "=cat/pkg-1-2*",
            "=dev-lang/rust-1.70.0_extra",
            "dev-lang/rust:::",
            "dev-lang/rust*",
            "=dev-lang/rust*",
            "=dev-lang/rust-1.*",
            "dev-lang/rust[]",
            "dev-lang/rust[,]",
            "=kde-frameworks/*-6*::gentoo",
        ];

        for atom_str in invalid_atoms {
            assert!(Atom::new(atom_str).is_err(), "{atom_str} should be invalid");
        }
    }

    #[test]
    fn test_atom_qualified_name() {
        assert_eq!(
            Atom {
                category: Exact("dev-lang".into()),
                package: Exact("rust".into()),
                ..Default::default()
            }
            .qualified_name(),
            "dev-lang/rust"
        );
        assert_eq!(
            Atom {
                ..Default::default()
            }
            .qualified_name(),
            "*/*"
        );
        assert_eq!(
            Atom {
                category: Exact("dev-lang".into()),
                package: Any,
                ..Default::default()
            }
            .qualified_name(),
            "dev-lang/*"
        );
    }

    #[test]
    fn test_atom_display() {
        let test_data = [
            (
                Atom {
                    category: Exact("dev-lang".into()),
                    package: Exact("python".into()),
                    variant: AtomVariant::Simple,
                    repo: Some("gentoo".parse().unwrap()),
                    ..Default::default()
                },
                "dev-lang/python::gentoo",
            ),
            (
                Atom {
                    operator: Some(AtomOperator::Equal),
                    category: Exact("sys-apps".into()),
                    package: Exact("attr".into()),
                    version: PackageVersion::try_from("2.5.2-r1").ok(),
                    variant: AtomVariant::VersionOperator,
                    ..Default::default()
                },
                "=sys-apps/attr-2.5.2-r1",
            ),
            (
                Atom {
                    operator: Some(AtomOperator::GreaterEqual),
                    category: Exact("dev-lang".into()),
                    package: Exact("rust".into()),
                    version: PackageVersion::try_from("1.70.0_beta_p11-r2").ok(),
                    slot: Some(PackageSlot::Eq("1.70".into())),
                    repo: Some("gentoo".parse().unwrap()),
                    variant: AtomVariant::VersionOperator,
                    use_deps: vec!["clippy".parse().unwrap()],
                },
                ">=dev-lang/rust-1.70.0_beta_p11-r2:1.70::gentoo[clippy]",
            ),
            (
                Atom {
                    operator: Some(AtomOperator::Equal),
                    category: Exact("dev-libs".into()),
                    package: Exact("libffi".into()),
                    version: PackageVersion::try_from("3.5").ok(),
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

    #[test]
    fn test_name_match_fmt() {
        assert_eq!(
            AtomIdent::<CatName>::Exact("sys-libs".into()).to_string(),
            "sys-libs"
        );
        assert_eq!(Any::<CatName>.to_string(), "*");
    }
}
