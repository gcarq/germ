use crate::deps::DepExpression;
use crate::deps::atom::Atom;
use crate::deps::useflag::{PrefixedUseFlag, UseFlag};
use crate::eapi::Eapi;
use crate::package::slot::PackageSlot;
use crate::repository::Eclass;
use crate::types::FxHashMap;
use anyhow::bail;
use rkyv::{Archive, Deserialize, Serialize};
use std::{fmt, fs, io, path::Path, str::FromStr};
use thiserror::Error;

/// Errors returned when constructing package metadata from ebuild output.
#[derive(Debug, Error)]
pub enum PackageMetadataError {
    #[error("required metadata variable '{field}' is missing")]
    Missing { field: &'static str },

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
    pub homepage: Vec<String>,
    pub src_uri: Vec<String>,
    pub license: Vec<String>,
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
    /// Builds package metadata from the given `map`.
    ///
    /// Returns a [`PackageMetadataError`] if a required variable is missing or invalid.
    pub fn from_map(map: FxHashMap<&str, &str>) -> Result<Self, PackageMetadataError> {
        fn required<'a>(
            field: &'static str,
            map: &'a FxHashMap<&str, &str>,
        ) -> Result<&'a str, PackageMetadataError> {
            map.get(field)
                .copied()
                .ok_or(PackageMetadataError::Missing { field })
        }

        const fn invalid(field: &'static str, source: anyhow::Error) -> PackageMetadataError {
            PackageMetadataError::Invalid { field, source }
        }

        let metadata = Self::default()
            .eapi(required("EAPI", &map)?)
            .map_err(|source| invalid("EAPI", source))?
            .description(required("DESCRIPTION", &map)?)
            .homepage(required("HOMEPAGE", &map)?)
            .src_uri(required("SRC_URI", &map)?)
            .license(required("LICENSE", &map)?)
            .keywords(required("KEYWORDS", &map)?)
            .inherit(required("INHERIT", &map)?)
            .restrict(required("RESTRICT", &map)?)
            .map_err(|source| invalid("RESTRICT", source))?
            .defined_phases(required("DEFINED_PHASES", &map)?)
            .iuse(required("IUSE", &map)?)
            .map_err(|source| invalid("IUSE", source))?
            .required_use(required("REQUIRED_USE", &map)?)
            .map_err(|source| invalid("REQUIRED_USE", source))?
            .slot(required("SLOT", &map)?)
            .map_err(|source| invalid("SLOT", source))?
            .depend(required("DEPEND", &map)?)
            .map_err(|source| invalid("DEPEND", source))?
            .bdepend(required("BDEPEND", &map)?)
            .map_err(|source| invalid("BDEPEND", source))?
            .idepend(required("IDEPEND", &map)?)
            .map_err(|source| invalid("IDEPEND", source))?
            .pdepend(required("PDEPEND", &map)?)
            .map_err(|source| invalid("PDEPEND", source))?
            .rdepend(required("RDEPEND", &map)?)
            .map_err(|source| invalid("RDEPEND", source))?;
        Ok(metadata)
    }

    /// Builds [`PackageMetadata`] from the given VDB package `path`.
    pub fn from_vdb_path(path: &Path) -> anyhow::Result<Self> {
        fn read_meta(path: &Path) -> anyhow::Result<String> {
            match fs::read_to_string(path) {
                Ok(content) => Ok(content.trim().to_string()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
                Err(e) => bail!("failed to read metadata from '{}': {e}", path.display()),
            }
        }

        PackageMetadata::default()
            .eapi(&read_meta(&path.join("EAPI"))?)?
            .description(&read_meta(&path.join("DESCRIPTION"))?)
            .homepage(&read_meta(&path.join("HOMEPAGE"))?)
            .license(&read_meta(&path.join("LICENSE"))?)
            .keywords(&read_meta(&path.join("KEYWORDS"))?)
            .inherit(&read_meta(&path.join("INHERIT"))?)
            .restrict(&read_meta(&path.join("RESTRICT"))?)?
            .defined_phases(&read_meta(&path.join("DEFINED_PHASES"))?)
            .iuse(&read_meta(&path.join("IUSE"))?)?
            .required_use(&read_meta(&path.join("REQUIRED_USE"))?)?
            .slot(&read_meta(&path.join("SLOT"))?)?
            .depend(&read_meta(&path.join("DEPEND"))?)?
            .bdepend(&read_meta(&path.join("BDEPEND"))?)?
            .idepend(&read_meta(&path.join("IDEPEND"))?)?
            .pdepend(&read_meta(&path.join("PDEPEND"))?)?
            .rdepend(&read_meta(&path.join("RDEPEND"))?)
    }

    pub fn eapi(mut self, value: &str) -> anyhow::Result<Self> {
        self.eapi = value.parse()?;
        Ok(self)
    }

    pub fn description(mut self, value: &str) -> Self {
        self.description = value.to_string();
        self
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

    pub fn slot(mut self, value: &str) -> anyhow::Result<Self> {
        self.slot = value.parse()?;
        Ok(self)
    }

    pub fn depend(mut self, value: &str) -> anyhow::Result<Self> {
        self.depend = DepExpression::parse(value)?;
        Ok(self)
    }

    pub fn bdepend(mut self, value: &str) -> anyhow::Result<Self> {
        self.bdepend = DepExpression::parse(value)?;
        Ok(self)
    }

    pub fn idepend(mut self, value: &str) -> anyhow::Result<Self> {
        self.idepend = DepExpression::parse(value)?;
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
            "DESCRIPTION=Example python package",
            "KEYWORDS=amd64 x86",
            "INHERITED= toolchain-funcs bash-completion-r1 eapi9-ver edo linux-info systemd",
            "IUSE=examples +ipv6",
            "REQUIRED_USE=^^ ( python_single_target_python3_11 )",
            "PDEPEND=",
            "BDEPEND= \tpython_single_target_python3_11? ( \t dev-python/setuptools \t )",
            "EAPI=8",
            "PROPERTIES=",
            "DEFINED_PHASES=",
            "IDEPEND=",
            "INHERIT= bash-completion-r1 eapi9-ver edo linux-info systemd",
        ]
        .iter()
        .filter_map(|d| d.split_once('='))
        .collect()
    }

    #[test]
    fn test_metadata_from_map_ok() {
        let metadata = PackageMetadata::from_map(metadata_map());
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
        assert_eq!(metadata.idepend.to_string(), "");
        assert_eq!(metadata.pdepend.to_string(), "");
        assert_eq!(
            metadata.rdepend.to_string(),
            "python_single_target_python3_11? ( dev-lang/python:3.11 )"
        );
    }

    #[test]
    fn test_metadata_from_map_missing() {
        let mut data = metadata_map();
        data.remove("SLOT");
        assert!(matches!(
            PackageMetadata::from_map(data).unwrap_err(),
            PackageMetadataError::Missing { field: "SLOT" }
        ));
    }

    #[test]
    fn test_metadata_from_map_invalid() {
        let mut data = metadata_map();
        data.insert("SLOT", "invalid/slot/value");
        assert!(matches!(
            PackageMetadata::from_map(data).unwrap_err(),
            PackageMetadataError::Invalid { field: "SLOT", .. }
        ));
    }
}
