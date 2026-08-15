use crate::deps::DepExpression;
use crate::deps::atom::Atom;
use crate::deps::useflag::{PrefixedUseFlag, UseFlag};
use crate::eapi::Eapi;
use crate::package::slot::PackageSlot;
use crate::repository::Eclass;
use crate::types::FxHashMap;
use anyhow::{anyhow, bail};
use rkyv::{Archive, Deserialize, Serialize};
use std::{fmt, fs, io, path::Path, str::FromStr};
use thiserror::Error;

/// Errors returned when constructing package metadata from ebuild output.
#[derive(Debug, Error)]
pub enum PackageMetadataError {
    #[error("required metadata variable '{0}' is empty")]
    Empty(&'static str),

    #[error("required metadata variable '{0}' is missing")]
    Missing(&'static str),

    #[error("invalid value for metadata variable '{field}'")]
    Invalid {
        field: &'static str,
        #[source]
        source: anyhow::Error,
    },
}

/// Holds all metadata of a [`Package`].
/// TODO: parse eclasses
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Clone, Default, Debug)]
pub struct PackageMetadata {
    pub eapi: Eapi,
    pub description: String,
    // TODO: should be parsed as DepExpression
    pub homepage: Vec<String>,
    // TODO: should be parsed as DepExpression
    pub src_uri: Vec<String>,
    // TODO: enforce valid license identifiers
    // and this should be parsed as DepExpression
    pub license: Vec<String>,
    pub properties: Vec<String>,
    // TODO: enforce valid keywords
    pub keywords: Vec<String>,
    pub inherit: Vec<String>,
    pub restrict: DepExpression<UseFlag>,
    pub defined_phases: Vec<String>,
    pub iuse: Vec<PrefixedUseFlag>,
    pub required_use: DepExpression<UseFlag>,
    pub slot: PackageSlot,
    pub depend: DepExpression<Atom>,
    pub bdepend: DepExpression<Atom>,
    pub idepend: DepExpression<Atom>,
    pub pdepend: DepExpression<Atom>,
    pub rdepend: DepExpression<Atom>,
    pub eclasses: Vec<Eclass>,
}

impl PackageMetadata {
    /// Builds package metadata from the given `map` and validates it against `eapi`.
    ///
    /// Returns a [`PackageMetadataError`] if a required variable is missing or invalid.
    pub fn from_map(map: FxHashMap<&str, &str>, eapi: &Eapi) -> Result<Self, PackageMetadataError> {
        let metadata = Self::default()
            .eapi(map.get("EAPI").copied().unwrap_or(""))
            .map_err(|err| invalid("EAPI", err))?;

        // The parsed EAPI must the sourced EAPI, see PMS 7.3.1
        if eapi != &metadata.eapi {
            return Err(invalid(
                "EAPI",
                anyhow!("expected '{eapi}', found '{}'", metadata.eapi),
            ));
        }

        let metadata = metadata
            .description(map.get("DESCRIPTION").copied())?
            .homepage(map.get("HOMEPAGE").copied().unwrap_or(""))
            .src_uri(map.get("SRC_URI").copied().unwrap_or(""))
            .license(map.get("LICENSE").copied().unwrap_or(""))
            .properties(map.get("PROPERTIES").copied().unwrap_or(""))
            .keywords(map.get("KEYWORDS").copied().unwrap_or(""))
            .inherit(map.get("INHERIT").copied().unwrap_or(""))
            .restrict(map.get("RESTRICT").copied().unwrap_or(""))
            .map_err(|err| invalid("RESTRICT", err))?
            .defined_phases(map.get("DEFINED_PHASES").copied().unwrap_or(""))
            .iuse(map.get("IUSE").copied().unwrap_or(""))
            .map_err(|err| invalid("IUSE", err))?
            .required_use(map.get("REQUIRED_USE").copied().unwrap_or(""))
            .map_err(|err| invalid("REQUIRED_USE", err))?
            .slot(map.get("SLOT").copied())?
            .depend(map.get("DEPEND").copied().unwrap_or(""))
            .map_err(|err| invalid("DEPEND", err))?
            .bdepend(map.get("BDEPEND").copied().unwrap_or(""), eapi)
            .map_err(|err| invalid("BDEPEND", err))?
            .idepend(map.get("IDEPEND").copied().unwrap_or(""), eapi)
            .map_err(|err| invalid("IDEPEND", err))?
            .pdepend(map.get("PDEPEND").copied().unwrap_or(""))
            .map_err(|err| invalid("PDEPEND", err))?
            .rdepend(map.get("RDEPEND").copied().unwrap_or(""))
            .map_err(|err| invalid("RDEPEND", err))?;
        Ok(metadata)
    }

    /// Builds [`PackageMetadata`] from the given VDB package `path`.
    pub fn from_vdb_path(path: &Path) -> anyhow::Result<Self> {
        fn read_meta(path: &Path) -> anyhow::Result<String> {
            match fs::read_to_string(path) {
                Ok(content) => Ok(content),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
                Err(e) => bail!("failed to read metadata from '{}': {e}", path.display()),
            }
        }

        let metadata = Self::default()
            .eapi(read_meta(&path.join("EAPI"))?.trim())?
            .description(Some(read_meta(&path.join("DESCRIPTION"))?.trim()))?
            .homepage(read_meta(&path.join("HOMEPAGE"))?.trim())
            .license(read_meta(&path.join("LICENSE"))?.trim())
            .properties(read_meta(&path.join("PROPERTIES"))?.trim())
            .keywords(read_meta(&path.join("KEYWORDS"))?.trim())
            .inherit(read_meta(&path.join("INHERIT"))?.trim())
            .restrict(read_meta(&path.join("RESTRICT"))?.trim())?
            .defined_phases(read_meta(&path.join("DEFINED_PHASES"))?.trim())
            .iuse(read_meta(&path.join("IUSE"))?.trim())?
            .required_use(read_meta(&path.join("REQUIRED_USE"))?.trim())?
            .slot(Some(read_meta(&path.join("SLOT"))?.trim()))?
            .depend(read_meta(&path.join("DEPEND"))?.trim())?;

        let eapi = metadata.eapi;
        metadata
            .bdepend(read_meta(&path.join("BDEPEND"))?.trim(), &eapi)?
            .idepend(read_meta(&path.join("IDEPEND"))?.trim(), &eapi)?
            .pdepend(read_meta(&path.join("PDEPEND"))?.trim())?
            .rdepend(read_meta(&path.join("RDEPEND"))?.trim())
    }

    pub fn eapi(mut self, value: &str) -> anyhow::Result<Self> {
        self.eapi = match value.is_empty() {
            true => Eapi::Zero,
            false => value.parse()?,
        };
        Ok(self)
    }

    pub fn description(mut self, value: Option<&str>) -> Result<Self, PackageMetadataError> {
        let value = required("DESCRIPTION", value)?;
        if value.is_empty() {
            return Err(PackageMetadataError::Empty("DESCRIPTION"));
        }
        self.description = value.to_string();
        Ok(self)
    }

    pub fn homepage(mut self, value: &str) -> Self {
        self.homepage = Self::parse_value(value);
        self
    }

    pub fn src_uri(mut self, value: &str) -> Self {
        self.src_uri = Self::parse_value(value);
        self
    }

    pub fn license(mut self, value: &str) -> Self {
        self.license = Self::parse_value(value);
        self
    }

    pub fn properties(mut self, value: &str) -> Self {
        self.properties = Self::parse_value(value);
        self
    }

    pub fn keywords(mut self, value: &str) -> Self {
        self.keywords = Self::parse_value(value);
        self
    }

    pub fn inherit(mut self, value: &str) -> Self {
        self.inherit = Self::parse_value(value);
        self
    }

    pub fn restrict(mut self, value: &str) -> anyhow::Result<Self> {
        self.restrict = DepExpression::parse(value)?;
        Ok(self)
    }

    pub fn defined_phases(mut self, value: &str) -> Self {
        self.defined_phases = Self::parse_value(value);
        self
    }

    pub fn iuse(mut self, value: &str) -> anyhow::Result<Self> {
        self.iuse = value
            .split_whitespace()
            .map(PrefixedUseFlag::from_str)
            .collect::<anyhow::Result<_>>()?;
        Ok(self)
    }

    pub fn required_use(mut self, value: &str) -> anyhow::Result<Self> {
        self.required_use = DepExpression::parse(value)?;
        Ok(self)
    }

    pub fn slot(mut self, value: Option<&str>) -> Result<Self, PackageMetadataError> {
        let value = required("SLOT", value)?;
        if value.is_empty() {
            return Err(PackageMetadataError::Empty("SLOT"));
        }
        self.slot = value.parse().map_err(|err| invalid("SLOT", err))?;
        Ok(self)
    }

    pub fn depend(mut self, value: &str) -> anyhow::Result<Self> {
        self.depend = DepExpression::parse(value)?;
        Ok(self)
    }

    pub fn bdepend(mut self, value: &str, eapi: &Eapi) -> anyhow::Result<Self> {
        if eapi.supports_bdepend() {
            self.bdepend = DepExpression::parse(value)?;
        }
        Ok(self)
    }

    pub fn idepend(mut self, value: &str, eapi: &Eapi) -> anyhow::Result<Self> {
        if eapi.supports_idepend() {
            self.idepend = DepExpression::parse(value)?;
        }
        Ok(self)
    }

    pub fn pdepend(mut self, value: &str) -> anyhow::Result<Self> {
        self.pdepend = DepExpression::parse(value)?;
        Ok(self)
    }

    pub fn rdepend(mut self, value: &str) -> anyhow::Result<Self> {
        self.rdepend = DepExpression::parse(value)?;
        Ok(self)
    }

    fn parse_value(value: &str) -> Vec<String> {
        value.split_whitespace().map(String::from).collect()
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
        writeln!(f, "INHERIT={}", self.inherit.join(" "))?;
        writeln!(
            f,
            "IUSE={}",
            self.iuse
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        )?;
        writeln!(f, "KEYWORDS={}", self.keywords.join(" "))?;
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

/// Returns the required metadata value or an error if it is missing.
fn required<'a>(
    field: &'static str,
    value: Option<&'a str>,
) -> Result<&'a str, PackageMetadataError> {
    value.ok_or(PackageMetadataError::Missing(field))
}

/// Helper function to create an [`PackageMetadataError::Invalid`]`.
const fn invalid(field: &'static str, source: anyhow::Error) -> PackageMetadataError {
    PackageMetadataError::Invalid { field, source }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata_map() -> FxHashMap<&'static str, &'static str> {
        [
            "DEPEND=",
            "RDEPEND= \tpython_single_target_python3_11? ( \t\t\tdev-lang/python:3.11 \t\t)",
            "SLOT=0",
            "SRC_URI=https://localhost/a https://localhost/b",
            "RESTRICT=",
            "HOMEPAGE=https://localhost",
            "LICENSE=GPL-3",
            "PROPERTIES=live test_network",
            "DESCRIPTION=Example python package",
            "KEYWORDS=amd64 x86",
            "INHERITED= toolchain-funcs bash-completion-r1 eapi9-ver edo linux-info systemd",
            "IUSE=examples +ipv6",
            "REQUIRED_USE=^^ ( python_single_target_python3_11 )",
            "PDEPEND=",
            "BDEPEND= \tpython_single_target_python3_11? ( \t dev-python/setuptools \t )",
            "EAPI=8",
            "DEFINED_PHASES=",
            "IDEPEND=dev-python/installer",
            "INHERIT= bash-completion-r1 eapi9-ver edo linux-info systemd",
        ]
        .iter()
        .filter_map(|d| d.split_once('='))
        .collect()
    }

    #[test]
    fn test_metadata_from_map_ok() {
        let metadata = PackageMetadata::from_map(metadata_map(), &Eapi::Eight);
        assert!(metadata.is_ok(), "metadata should be parsed successfully");

        let metadata = metadata.unwrap();
        assert_eq!(metadata.eapi, Eapi::Eight);
        assert_eq!(metadata.description, "Example python package");
        assert_eq!(metadata.homepage, vec!["https://localhost"]);
        assert_eq!(
            metadata.src_uri,
            vec!["https://localhost/a", "https://localhost/b"]
        );
        assert_eq!(metadata.license, vec!["GPL-3"]);
        assert_eq!(metadata.properties, vec!["live", "test_network"]);
        assert_eq!(metadata.keywords, vec!["amd64", "x86"]);
        assert_eq!(
            metadata.inherit,
            vec![
                "bash-completion-r1",
                "eapi9-ver",
                "edo",
                "linux-info",
                "systemd"
            ]
        );
        assert_eq!(metadata.restrict.to_string(), "");
        assert_eq!(metadata.defined_phases.len(), 0);
        assert_eq!(
            metadata.iuse,
            vec!["examples".parse().unwrap(), "+ipv6".parse().unwrap()]
        );
        assert_eq!(
            metadata.required_use.to_string(),
            "^^ ( python_single_target_python3_11 )"
        );
        assert_eq!(metadata.slot, PackageSlot::Eq("0".into()));
        assert_eq!(metadata.depend.to_string(), "");
        assert_eq!(
            metadata.bdepend.to_string(),
            "python_single_target_python3_11? ( dev-python/setuptools )"
        );
        assert_eq!(metadata.idepend.to_string(), "dev-python/installer");
        assert_eq!(metadata.pdepend.to_string(), "");
        assert_eq!(
            metadata.rdepend.to_string(),
            "python_single_target_python3_11? ( dev-lang/python:3.11 )"
        );
    }

    #[test]
    fn test_metadata_from_map_eapi_7() {
        let mut data = metadata_map();
        data.insert("EAPI", "7");
        data.insert("IDEPEND", "(");
        let metadata = PackageMetadata::from_map(data, &Eapi::Seven).unwrap();

        assert_eq!(
            metadata.bdepend.to_string(),
            "python_single_target_python3_11? ( dev-python/setuptools )"
        );
        assert_eq!(metadata.idepend.to_string(), "");
    }

    #[test]
    fn test_metadata_from_map_missing() {
        let mut data = metadata_map();
        data.remove("SLOT");
        assert!(matches!(
            PackageMetadata::from_map(data, &Eapi::Eight).unwrap_err(),
            PackageMetadataError::Missing("SLOT")
        ));
    }

    #[test]
    fn test_metadata_from_map_empty() {
        for field in ["DESCRIPTION", "SLOT"] {
            let mut data = metadata_map();
            data.insert(field, "");
            assert!(matches!(
                PackageMetadata::from_map(data, &Eapi::Eight).unwrap_err(),
                PackageMetadataError::Empty(name) if name == field
            ));
        }
    }

    #[test]
    fn test_metadata_from_map_invalid() {
        let mut data = metadata_map();
        data.insert("SLOT", "invalid/slot/value");
        assert!(matches!(
            PackageMetadata::from_map(data, &Eapi::Eight).unwrap_err(),
            PackageMetadataError::Invalid { field: "SLOT", .. }
        ));
    }
}
