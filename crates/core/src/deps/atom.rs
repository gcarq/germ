use crate::grammar::{CATEGORY, PACKAGE, REPOSITORY, REVISION, VERSION, VERSION_SUFFIXES};
use crate::package::cpv::CPV;
use crate::package::names::{CatName, PkgName};
use crate::package::slot::PackageSlot;
use crate::package::version::{PackageVersion, matches_wildcard};
use crate::repository::RepoName;
use crate::useflag::UseDep;
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

/// Represents a Portage package atom.
///
/// An atom can match one or more [`Package`] and is used for
/// calculating dependencies between packages.
#[derive(
    Archive, Serialize, Deserialize, Default, Clone, PartialEq, Eq, Ord, PartialOrd, Hash, Debug,
)]
pub struct Atom {
    kind: AtomKind,
    slot: Option<PackageSlot>,
    repo: Option<RepoName>,
    use_deps: Vec<UseDep>,
}

impl Atom {
    /// Creates an [`Atom`] from the given `atom` str.
    pub fn new(atom: &str) -> anyhow::Result<Self> {
        let Some(captures) = ATOM_RE.captures(atom)? else {
            bail!("'{atom}' is not a valid atom");
        };

        Self::from_regex_capture(&captures)
            .with_context(|| format!("unable to parse atom '{atom}'"))
    }

    /// Returns the qualified name for this atom in the format
    /// `category/name` e.g. `app-editors/vim`.
    pub fn qualified_name(&self) -> String {
        match &self.kind {
            AtomKind::Basic { category, package } => format!(
                "{}/{}",
                category.as_ref().map_or("*", CatName::as_str),
                package.as_ref().map_or("*", PkgName::as_str)
            ),
            AtomKind::Versioned {
                category, package, ..
            } => format!("{category}/{package}"),
        }
    }

    /// Returns the exact category, or `None` for a wildcard category.
    pub const fn category(&self) -> Option<&CatName> {
        match &self.kind {
            AtomKind::Basic { category, .. } => category.as_ref(),
            AtomKind::Versioned { category, .. } => Some(category),
        }
    }

    /// Returns the exact package name, or `None` for a wildcard package.
    pub const fn package(&self) -> Option<&PkgName> {
        match &self.kind {
            AtomKind::Basic { package, .. } => package.as_ref(),
            AtomKind::Versioned { package, .. } => Some(package),
        }
    }

    /// Returns the repository restriction, if any.
    pub const fn repo(&self) -> Option<&RepoName> {
        self.repo.as_ref()
    }

    /// Returns the slot restriction, if any.
    pub const fn slot(&self) -> Option<&PackageSlot> {
        self.slot.as_ref()
    }

    /// Returns `true` if the given `cpv` matches this atom.
    pub fn matches(&self, cpv: &CPV) -> bool {
        match &self.kind {
            AtomKind::Basic { category, package } => match (category, package) {
                (Some(cat), Some(pkg)) => cat == cpv.category() && cpv.package() == pkg,
                (Some(cat), None) => cat == cpv.category(),
                (None, Some(pkg)) => pkg == cpv.package(),
                (None, None) => true,
            },
            AtomKind::Versioned {
                category,
                package,
                constraint,
            } => {
                category == cpv.category()
                    && package == cpv.package()
                    && constraint.matches(cpv.version())
            }
        }
    }

    /// Creates an [`Atom`] from the captures of the atom grammar.
    fn from_regex_capture(captures: &Captures<'_, str>) -> anyhow::Result<Self> {
        let kind = match Self::parse_version_constraint(captures)? {
            Some(constraint) => AtomKind::Versioned {
                category: Self::first_capture(captures, ["operator_category", "wildcard_category"])
                    .ok_or_else(|| anyhow!("atom missing <category>"))?
                    .parse()?,
                package: Self::first_capture(captures, ["operator_package", "wildcard_package"])
                    .ok_or_else(|| anyhow!("atom missing <package>"))?
                    .parse()?,
                constraint,
            },
            None => AtomKind::Basic {
                category: Self::parse_selector(&captures["simple_category"])?,
                package: Self::parse_selector(&captures["simple_package"])?,
            },
        };

        Ok(Self {
            kind,
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
                        .map(str::parse)
                        .collect::<anyhow::Result<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default(),
        })
    }

    /// Parses the version constraint based on the captured atom.
    fn parse_version_constraint(
        captures: &Captures<'_, str>,
    ) -> anyhow::Result<Option<VersionConstraint>> {
        if let Some(version) = captures.name("wildcard_version") {
            return Ok(Some(VersionConstraint::EqualWildcard(PackageVersion::new(
                version.as_str(),
                captures.name("wildcard_suffixes").map(|m| m.as_str()),
                captures.name("wildcard_revision").map(|m| m.as_str()),
            )?)));
        }

        let Some(operator) = captures.name("operator") else {
            return Ok(None);
        };
        let version = PackageVersion::new(
            captures
                .name("operator_version")
                .ok_or_else(|| anyhow!("atom missing <version>"))?
                .as_str(),
            captures.name("operator_suffixes").map(|m| m.as_str()),
            captures.name("operator_revision").map(|m| m.as_str()),
        )?;

        Ok(Some(match operator.as_str() {
            "=" => VersionConstraint::Equal(version),
            "~" => VersionConstraint::Approximate(version),
            ">" => VersionConstraint::Greater(version),
            ">=" => VersionConstraint::GreaterEqual(version),
            "<" => VersionConstraint::Less(version),
            "<=" => VersionConstraint::LessEqual(version),
            operator => bail!("invalid operator: {operator}"),
        }))
    }

    /// Parses an exact selector, or returns `None` for the wildcard selector.
    fn parse_selector<T>(value: &str) -> anyhow::Result<Option<T>>
    where
        T: FromStr<Err = anyhow::Error>,
    {
        match value {
            "*" => Ok(None),
            _ => Ok(Some(value.parse()?)),
        }
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
        Self::new(atom)
    }
}

impl fmt::Display for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            AtomKind::Basic { category, package } => write!(
                f,
                "{}/{}",
                category.as_ref().map_or("*", CatName::as_str),
                package.as_ref().map_or("*", PkgName::as_str)
            )?,
            AtomKind::Versioned {
                category,
                package,
                constraint,
            } => {
                write!(
                    f,
                    "{}{category}/{package}-{}",
                    constraint.operator(),
                    constraint.version()
                )?;
                if constraint.is_wildcard() {
                    f.write_char('*')?;
                }
            }
        }
        if let Some(slot) = &self.slot {
            write!(f, ":{slot}")?;
        }
        if let Some(repo) = &self.repo {
            write!(f, "::{repo}")?;
        }
        if !self.use_deps.is_empty() {
            f.write_char('[')?;
            for (i, use_dep) in self.use_deps.iter().enumerate() {
                if i > 0 {
                    f.write_char(',')?;
                }
                write!(f, "{use_dep}")?;
            }
            f.write_char(']')?;
        }
        Ok(())
    }
}

/// Classifies an [`Atom`] into two categories.
///
/// See `man 5 ebuild` for more information.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
enum AtomKind {
    Basic {
        category: Option<CatName>,
        package: Option<PkgName>,
    },
    Versioned {
        category: CatName,
        package: PkgName,
        constraint: VersionConstraint,
    },
}

impl Default for AtomKind {
    fn default() -> Self {
        Self::Basic {
            category: None,
            package: None,
        }
    }
}

/// Defines how a versioned [`Atom`] matches a [`PackageVersion`].
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
enum VersionConstraint {
    Equal(PackageVersion),
    EqualWildcard(PackageVersion),
    Approximate(PackageVersion),
    Greater(PackageVersion),
    GreaterEqual(PackageVersion),
    Less(PackageVersion),
    LessEqual(PackageVersion),
}

impl VersionConstraint {
    /// Returns `true` if the given `candidate` version matches.
    fn matches(&self, candidate: &PackageVersion) -> bool {
        match self {
            Self::Equal(version) => candidate == version,
            Self::EqualWildcard(version) => matches_wildcard(version, candidate),
            Self::Approximate(version) => candidate.matches_approximate(version),
            Self::Greater(version) => candidate > version,
            Self::GreaterEqual(version) => candidate >= version,
            Self::Less(version) => candidate < version,
            Self::LessEqual(version) => candidate <= version,
        }
    }

    /// Returns the operator as `&str`.
    const fn operator(&self) -> &str {
        match self {
            Self::Equal(_) | Self::EqualWildcard(_) => "=",
            Self::Approximate(_) => "~",
            Self::Greater(_) => ">",
            Self::GreaterEqual(_) => ">=",
            Self::Less(_) => "<",
            Self::LessEqual(_) => "<=",
        }
    }

    /// Returns the inner [`PackageVersion`].
    const fn version(&self) -> &PackageVersion {
        match self {
            Self::Equal(version)
            | Self::EqualWildcard(version)
            | Self::Approximate(version)
            | Self::Greater(version)
            | Self::GreaterEqual(version)
            | Self::Less(version)
            | Self::LessEqual(version) => version,
        }
    }

    const fn is_wildcard(&self) -> bool {
        matches!(self, Self::EqualWildcard(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::cpv;

    #[test]
    fn test_atom_parse() {
        for atom in [
            "dev-lang/rust",
            "*/*",
            "*/rust",
            "dev-lang/*",
            "dev-lang/rust:1.92.0",
            "cat/foo-r2",
            "cat/foo-::repo-",
            "net-misc/*:*::gentoo",
            "x11-drivers/nvidia-drivers:0/390",
            "sys-libs/glibc[audit,caps(-)]",
            "=sys-apps/memtest86+-7.2.0",
            "=cat/pkg-1-r2",
            ">=sys-apps/sed-4.8",
            "<net-misc/dhcp-3",
            "<=net-misc/dhcp-3.0_p2",
            ">dev-lang/python-3.14.3_beta-r2:3.14",
            "~dev-lang/rust-1.70.0:1.70.0/1::gentoo",
            ">=sys-libs/glibc-2.41-r10:2.2::gentoo[cet,clang]",
            "=dev-libs/glib-2*",
            "=dev-lang/rust-1.70*:1.70.0",
            "=kde-frameworks/kwindowsystem-6*:6/6.23::gentoo",
            "=app-arch/7zip-26*[rar]",
        ] {
            let parsed = Atom::new(atom).unwrap();
            assert_eq!(parsed.to_string(), atom);
        }
    }

    #[test]
    fn test_atom_default() {
        assert_eq!(Atom::default().to_string(), "*/*");
    }

    #[test]
    fn test_atom_matches_cpv() {
        let cpv = cpv("sys-devel", "gcc", "15.2.1_p20251122-r1");
        for atom in [
            "sys-devel/gcc",
            "=sys-devel/gcc-15*",
            "=sys-devel/gcc-15.2*",
            "=sys-devel/gcc-15.2.1*",
            "=sys-devel/gcc-15.2.1_p20251122-r1",
            ">sys-devel/gcc-15",
            ">=sys-devel/gcc-15.2.1",
            "<sys-devel/gcc-16",
            "<=sys-devel/gcc-15.2.2_p20260101",
            "~sys-devel/gcc-15.2.1_p20251122",
        ] {
            let atom = Atom::new(atom).unwrap();
            assert!(atom.matches(&cpv), "{atom} should match {cpv}");
        }
    }

    #[test]
    fn test_atom_excludes_cpv() {
        let cpv = cpv("sys-devel", "gcc", "15.2.1_p20251122-r1");
        for atom in [
            "sys-devel/binutils",
            "virtual/gcc",
            "<sys-devel/gcc-15",
            "<=sys-devel/gcc-15.2.1",
            ">sys-devel/gcc-16",
            ">=sys-devel/gcc-15.2.2_p20251122-r2",
            "=sys-devel/gcc-15.2.2",
            "=sys-devel/gcc-15.2.2*",
            "=sys-devel/gcc-15.2.1_p20260330",
            "~sys-devel/gcc-15.3",
            "~sys-devel/gcc-15",
            "~sys-devel/gcc-15.2",
            "~sys-devel/gcc-15.2.1",
            "~sys-devel/gcc-15.2.1_p20260101",
        ] {
            let atom = Atom::new(atom).unwrap();
            assert!(!atom.matches(&cpv), "{atom} shouldn't match {cpv}");
        }
    }

    #[test]
    fn test_atom_parse_error() {
        for atom in [
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
        ] {
            assert!(Atom::new(atom).is_err());
        }
    }

    #[test]
    fn test_atom_qualified_name() {
        for (atom, expected) in [
            ("dev-lang/rust", "dev-lang/rust"),
            ("*/*", "*/*"),
            ("dev-lang/*", "dev-lang/*"),
            ("=dev-lang/rust-1", "dev-lang/rust"),
        ] {
            assert_eq!(Atom::new(atom).unwrap().qualified_name(), expected);
        }
    }

    #[test]
    fn test_atom_use_deps() {
        let atom = Atom::new("cat/pkg[foo,-bar,!baz?,qux(-)=]").unwrap();
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
            assert!(Atom::new(atom).is_err());
        }
    }
}
