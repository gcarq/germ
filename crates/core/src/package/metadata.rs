use crate::deps::atom::Atom;
use crate::deps::{DepExpression, ExpressionItem, ExpressionKind};
use crate::eapi::Eapi;
use crate::keyword::Keyword;
use crate::package::slot::PackageSlot;
use crate::repository::Eclass;
use crate::types::FxHashMap;
use crate::useflag::{IUseEntry, UseFlag};
use rkyv::{Archive, Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

/// Identifies metadata variable names.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MetaVar {
    Eapi,
    Description,
    Homepage,
    SrcUri,
    License,
    Properties,
    Keywords,
    Inherited,
    Restrict,
    DefinedPhases,
    IUse,
    RequiredUse,
    Slot,
    Depend,
    BDepend,
    IDepend,
    PDepend,
    RDepend,
}

impl MetaVar {
    /// All metadata variables, in the order they are read from a metadata source.
    pub const ALL: [Self; 18] = [
        Self::Eapi,
        Self::Description,
        Self::Homepage,
        Self::SrcUri,
        Self::License,
        Self::Properties,
        Self::Keywords,
        Self::Inherited,
        Self::Restrict,
        Self::DefinedPhases,
        Self::IUse,
        Self::RequiredUse,
        Self::Slot,
        Self::Depend,
        Self::BDepend,
        Self::IDepend,
        Self::PDepend,
        Self::RDepend,
    ];

    /// Returns the name this variable is stored under in a metadata source.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Eapi => "EAPI",
            Self::Description => "DESCRIPTION",
            Self::Homepage => "HOMEPAGE",
            Self::SrcUri => "SRC_URI",
            Self::License => "LICENSE",
            Self::Properties => "PROPERTIES",
            Self::Keywords => "KEYWORDS",
            Self::Inherited => "INHERITED",
            Self::Restrict => "RESTRICT",
            Self::DefinedPhases => "DEFINED_PHASES",
            Self::IUse => "IUSE",
            Self::RequiredUse => "REQUIRED_USE",
            Self::Slot => "SLOT",
            Self::Depend => "DEPEND",
            Self::BDepend => "BDEPEND",
            Self::IDepend => "IDEPEND",
            Self::PDepend => "PDEPEND",
            Self::RDepend => "RDEPEND",
        }
    }
}

impl fmt::Display for MetaVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Errors returned when parsing package metadata variables.
#[derive(Debug, Error)]
pub enum PackageMetadataError {
    #[error("required metadata variable '{0}' is empty")]
    Empty(MetaVar),

    #[error("required metadata variable '{0}' is missing")]
    Missing(MetaVar),

    #[error("metadata EAPI '{found}' does not match the sourced EAPI '{expected}'")]
    EapiMismatch { expected: Eapi, found: Eapi },

    #[error("invalid value for metadata variable '{var}'")]
    Invalid {
        var: MetaVar,
        #[source]
        source: anyhow::Error,
    },
}

/// Holds validated metadata of a package.
/// TODO: parse eclasses
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Clone, Debug)]
pub struct PackageMetadata {
    eapi: Eapi,
    description: String,
    // TODO: should be parsed as DepExpression
    homepage: Vec<String>,
    // TODO: should be parsed as DepExpression
    src_uri: Vec<String>,
    // TODO: enforce valid license identifiers and parse as DepExpression
    license: Vec<String>,
    properties: Vec<String>,
    keywords: Vec<Keyword>,
    inherit: Vec<String>,
    // TODO: use a string instead of UseFlag
    restrict: DepExpression<UseFlag>,
    defined_phases: Vec<String>,
    iuse: Vec<IUseEntry>,
    required_use: DepExpression<UseFlag>,
    slot: PackageSlot,
    depend: DepExpression<Atom>,
    bdepend: DepExpression<Atom>,
    idepend: DepExpression<Atom>,
    pdepend: DepExpression<Atom>,
    rdepend: DepExpression<Atom>,
    eclasses: Vec<Eclass>,
}

impl PackageMetadata {
    pub const fn eapi(&self) -> Eapi {
        self.eapi
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn homepage(&self) -> &[String] {
        &self.homepage
    }

    pub fn src_uri(&self) -> &[String] {
        &self.src_uri
    }

    pub fn license(&self) -> &[String] {
        &self.license
    }

    pub fn properties(&self) -> &[String] {
        &self.properties
    }

    pub fn keywords(&self) -> &[Keyword] {
        &self.keywords
    }

    pub fn inherit(&self) -> &[String] {
        &self.inherit
    }

    pub const fn restrict(&self) -> &DepExpression<UseFlag> {
        &self.restrict
    }

    pub fn defined_phases(&self) -> &[String] {
        &self.defined_phases
    }

    pub fn iuse(&self) -> &[IUseEntry] {
        &self.iuse
    }

    pub const fn required_use(&self) -> &DepExpression<UseFlag> {
        &self.required_use
    }

    pub const fn slot(&self) -> &PackageSlot {
        &self.slot
    }

    pub const fn depend(&self) -> &DepExpression<Atom> {
        &self.depend
    }

    pub const fn bdepend(&self) -> &DepExpression<Atom> {
        &self.bdepend
    }

    pub const fn idepend(&self) -> &DepExpression<Atom> {
        &self.idepend
    }

    pub const fn pdepend(&self) -> &DepExpression<Atom> {
        &self.pdepend
    }

    pub const fn rdepend(&self) -> &DepExpression<Atom> {
        &self.rdepend
    }

    pub fn eclasses(&self) -> &[Eclass] {
        &self.eclasses
    }

    /// Builds validated metadata from the given raw `variables`.
    ///
    /// Returns a [`PackageMetadataError`] if a required variable is missing or invalid.
    /// When `sourced_eapi` is given, the parsed EAPI must match it, see PMS 7.3.1.
    pub(crate) fn from_raw(
        vars: &RawPackageMetadata<'_>,
        sourced_eapi: Option<Eapi>,
    ) -> Result<Self, PackageMetadataError> {
        let eapi = match optional(vars.get(MetaVar::Eapi)) {
            "" => Eapi::Zero,
            value => Eapi::new(value).map_err(|err| invalid(MetaVar::Eapi, err.into()))?,
        };

        // The parsed EAPI must match the sourced EAPI, see PMS 7.3.1
        if let Some(sourced_eapi) = sourced_eapi
            && sourced_eapi != eapi
        {
            return Err(PackageMetadataError::EapiMismatch {
                expected: sourced_eapi,
                found: eapi,
            });
        }

        Ok(Self {
            eapi,
            description: vars.required(MetaVar::Description)?.to_owned(),
            homepage: vars.words(MetaVar::Homepage)?,
            src_uri: vars.words(MetaVar::SrcUri)?,
            license: vars.words(MetaVar::License)?,
            properties: vars.words(MetaVar::Properties)?,
            keywords: vars.words(MetaVar::Keywords)?,
            inherit: vars.words(MetaVar::Inherited)?,
            restrict: vars.expression(eapi, ExpressionKind::Restrict, MetaVar::Restrict)?,
            defined_phases: vars.words(MetaVar::DefinedPhases)?,
            iuse: vars.words(MetaVar::IUse)?,
            required_use: vars.expression(
                eapi,
                ExpressionKind::RequiredUse,
                MetaVar::RequiredUse,
            )?,
            slot: vars
                .required(MetaVar::Slot)?
                .parse()
                .map_err(|err| invalid(MetaVar::Slot, err))?,
            depend: vars.expression(eapi, ExpressionKind::Dependency, MetaVar::Depend)?,
            bdepend: eapi
                .supports_bdepend()
                .then(|| vars.expression(eapi, ExpressionKind::Dependency, MetaVar::BDepend))
                .transpose()?
                .unwrap_or_default(),
            idepend: eapi
                .supports_idepend()
                .then(|| vars.expression(eapi, ExpressionKind::Dependency, MetaVar::IDepend))
                .transpose()?
                .unwrap_or_default(),
            pdepend: vars.expression(eapi, ExpressionKind::Dependency, MetaVar::PDepend)?,
            rdepend: vars.expression(eapi, ExpressionKind::Dependency, MetaVar::RDepend)?,
            eclasses: Vec::new(),
        })
    }
}

impl fmt::Display for PackageMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "BDEPEND={}", self.bdepend)?;
        writeln!(f, "DEFINED_PHASES={}", self.defined_phases.join(" "))?;
        writeln!(f, "DEPEND={}", self.depend)?;
        writeln!(f, "DESCRIPTION={}", self.description)?;
        writeln!(f, "EAPI={}", self.eapi)?;
        writeln!(f, "HOMEPAGE={}", self.homepage.join(" "))?;
        writeln!(f, "IDEPEND={}", self.idepend)?;
        writeln!(f, "INHERITED={}", self.inherit.join(" "))?;
        writeln!(
            f,
            "IUSE={}",
            self.iuse
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        )?;
        writeln!(
            f,
            "KEYWORDS={}",
            self.keywords
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        )?;
        writeln!(f, "LICENSE={}", self.license.join(" "))?;
        writeln!(f, "PDEPEND={}", self.pdepend)?;
        writeln!(f, "PROPERTIES={}", self.properties.join(" "))?;
        writeln!(f, "RDEPEND={}", self.rdepend)?;
        writeln!(f, "RESTRICT={}", self.restrict)?;
        writeln!(f, "REQUIRED_USE={}", self.required_use)?;
        writeln!(f, "SLOT={}", self.slot)?;
        writeln!(f, "SRC_URI={}", self.src_uri.join(" "))?;
        writeln!(f, "_eclasses_=TODO")?;
        writeln!(f, "_md5_=TODO")?;
        Ok(())
    }
}

/// Raw metadata variables collected from a package metadata source.
#[derive(Debug)]
pub(crate) struct RawPackageMetadata<'a> {
    values: FxHashMap<Cow<'a, str>, Cow<'a, str>>,
}

impl<'a> RawPackageMetadata<'a> {
    /// Returns the value stored for the given metadata `var`, if any.
    fn get(&self, var: MetaVar) -> Option<&str> {
        self.values.get(var.name()).map(AsRef::as_ref)
    }

    /// Returns the trimmed value of `var`, which must be present and non-empty.
    fn required(&self, var: MetaVar) -> Result<&str, PackageMetadataError> {
        let value = self
            .get(var)
            .ok_or(PackageMetadataError::Missing(var))?
            .trim();
        if value.is_empty() {
            return Err(PackageMetadataError::Empty(var));
        }
        Ok(value)
    }

    /// Parses the value of `var` as a whitespace-separated list.
    fn words<T>(&self, var: MetaVar) -> Result<Vec<T>, PackageMetadataError>
    where
        T: FromStr,
        T::Err: Into<anyhow::Error>,
    {
        self.get(var)
            .unwrap_or_default()
            .split_whitespace()
            .map(|value| T::from_str(value).map_err(|err| invalid(var, err.into())))
            .collect()
    }

    /// Parses the value of `var` as a dependency expression of the given `kind`.
    fn expression<T: ExpressionItem>(
        &self,
        eapi: Eapi,
        kind: ExpressionKind,
        var: MetaVar,
    ) -> Result<DepExpression<T>, PackageMetadataError> {
        DepExpression::parse(eapi, kind, optional(self.get(var))).map_err(|err| invalid(var, err))
    }
}

impl<'a, K, V> FromIterator<(K, V)> for RawPackageMetadata<'a>
where
    K: Into<Cow<'a, str>>,
    V: Into<Cow<'a, str>>,
{
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let values = iter
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        Self { values }
    }
}

fn optional(value: Option<&str>) -> &str {
    value.unwrap_or_default().trim()
}

const fn invalid(var: MetaVar, source: anyhow::Error) -> PackageMetadataError {
    PackageMetadataError::Invalid { var, source }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(variables: &[(&'static str, &'static str)]) -> RawPackageMetadata<'static> {
        variables.iter().copied().collect()
    }

    fn raw_metadata() -> RawPackageMetadata<'static> {
        raw(&[
            ("DEPEND", ""),
            (
                "RDEPEND",
                " \tpython_single_target_python3_11? ( \t\t\tdev-lang/python:3.11 \t\t)",
            ),
            ("SLOT", "0/0"),
            ("SRC_URI", "https://localhost/a https://localhost/b"),
            ("RESTRICT", ""),
            ("HOMEPAGE", "https://localhost"),
            ("LICENSE", "GPL-3"),
            ("PROPERTIES", "live test_network"),
            ("DESCRIPTION", "Example python package"),
            ("KEYWORDS", "amd64 ~arm64"),
            ("IUSE", "examples +ipv6"),
            ("REQUIRED_USE", "^^ ( python_single_target_python3_11 )"),
            ("PDEPEND", ""),
            (
                "BDEPEND",
                " \tpython_single_target_python3_11? ( \t dev-python/setuptools \t )",
            ),
            ("EAPI", "8"),
            ("DEFINED_PHASES", ""),
            ("IDEPEND", "dev-python/installer"),
            (
                "INHERITED",
                " bash-completion-r1 eapi9-ver edo linux-info systemd",
            ),
        ])
    }

    #[test]
    fn test_metadata_from_raw_ok() {
        let metadata = PackageMetadata::from_raw(&raw_metadata(), None).unwrap();
        assert_eq!(metadata.eapi(), Eapi::Eight);
        assert_eq!(metadata.description(), "Example python package");
        assert_eq!(metadata.homepage(), ["https://localhost"]);
        assert_eq!(
            metadata.src_uri(),
            ["https://localhost/a", "https://localhost/b"]
        );
        assert_eq!(metadata.license(), ["GPL-3"]);
        assert_eq!(metadata.properties(), ["live", "test_network"]);
        assert_eq!(
            metadata.keywords(),
            &[
                Keyword::Stable("amd64".parse().unwrap()),
                Keyword::Testing("arm64".parse().unwrap())
            ]
        );
        assert_eq!(
            metadata.inherit(),
            &[
                "bash-completion-r1",
                "eapi9-ver",
                "edo",
                "linux-info",
                "systemd"
            ]
        );
        assert_eq!(metadata.restrict().to_string(), "");
        assert!(metadata.defined_phases().is_empty());
        assert_eq!(
            metadata.iuse(),
            &["examples".parse().unwrap(), "+ipv6".parse().unwrap()]
        );
        assert_eq!(
            metadata.required_use().to_string(),
            "^^ ( python_single_target_python3_11 )"
        );
        assert_eq!(metadata.slot().to_string(), "0/0");
        assert_eq!(metadata.depend().to_string(), "");
        assert_eq!(
            metadata.bdepend().to_string(),
            "python_single_target_python3_11? ( dev-python/setuptools )"
        );
        assert_eq!(metadata.idepend().to_string(), "dev-python/installer");
        assert_eq!(metadata.pdepend().to_string(), "");
        assert_eq!(
            metadata.rdepend().to_string(),
            "python_single_target_python3_11? ( dev-lang/python:3.11 )"
        );
    }

    #[test]
    fn test_metadata_from_raw_eapi_mismatch() {
        let variables = raw(&[
            ("EAPI", "7"),
            ("DESCRIPTION", "Test package"),
            ("SLOT", "0"),
        ]);
        assert!(matches!(
            PackageMetadata::from_raw(&variables, Some(Eapi::Eight)),
            Err(PackageMetadataError::EapiMismatch {
                expected: Eapi::Eight,
                found: Eapi::Seven
            })
        ));
    }

    #[test]
    fn test_metadata_raw_eapi_7() {
        let variables = raw(&[
            ("EAPI", "7"),
            ("DESCRIPTION", "Test package"),
            ("SLOT", "0"),
            ("BDEPEND", "dev-python/setuptools"),
            ("IDEPEND", "("),
        ]);
        let metadata = PackageMetadata::from_raw(&variables, None).unwrap();
        assert_eq!(metadata.bdepend().to_string(), "dev-python/setuptools");
        assert_eq!(metadata.idepend().to_string(), "");
    }

    #[test]
    fn test_metadata_raw_missing() {
        let variables = raw(&[("EAPI", "8"), ("DESCRIPTION", "Test package")]);
        assert!(matches!(
            PackageMetadata::from_raw(&variables, None),
            Err(PackageMetadataError::Missing(MetaVar::Slot))
        ));
    }

    #[test]
    fn test_metadata_raw_empty() {
        let description = raw(&[("EAPI", "8"), ("DESCRIPTION", ""), ("SLOT", "0")]);
        assert!(matches!(
            PackageMetadata::from_raw(&description, None),
            Err(PackageMetadataError::Empty(MetaVar::Description))
        ));

        let slot = raw(&[("EAPI", "8"), ("DESCRIPTION", "Test package"), ("SLOT", "")]);
        assert!(matches!(
            PackageMetadata::from_raw(&slot, None),
            Err(PackageMetadataError::Empty(MetaVar::Slot))
        ));
    }

    #[test]
    fn test_metadata_raw_invalid() {
        let slot = raw(&[
            ("EAPI", "8"),
            ("DESCRIPTION", "Test package"),
            ("SLOT", "invalid/slot/value"),
        ]);
        assert!(matches!(
            PackageMetadata::from_raw(&slot, None),
            Err(PackageMetadataError::Invalid {
                var: MetaVar::Slot,
                ..
            })
        ));

        let iuse = raw(&[
            ("EAPI", "8"),
            ("DESCRIPTION", "Test package"),
            ("SLOT", "0"),
            ("IUSE", "foo?"),
        ]);
        assert!(matches!(
            PackageMetadata::from_raw(&iuse, None),
            Err(PackageMetadataError::Invalid {
                var: MetaVar::IUse,
                ..
            })
        ));
    }
}
