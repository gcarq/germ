use crate::deps::UseFlag;
use crate::deps::atom::Atom;
use crate::deps::expr::{DepExpression, ExpressionItem};
use crate::eapi::Eapi;
use crate::package::slot::PackageSlot;
use crate::repository::eclass::Eclass;
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

/// Holds all metadata of a [`Package`].
/// TODO: parse eclasses
#[derive(Serialize, Deserialize, Clone, Eq, PartialEq)]
#[cfg_attr(test, derive(Default, Debug))]
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
    pub iuse: Vec<String>,
    pub required_use: DepExpression<UseFlag>,
    pub slot: PackageSlot,
    pub depend: DepExpression<Atom>,
    pub bdepend: DepExpression<Atom>,
    pub idepend: DepExpression<Atom>,
    pub pdepend: DepExpression<Atom>,
    pub rdepend: DepExpression<Atom>,
    pub eclasses: Vec<Eclass>,
    pub md5sum: String,
}

impl PackageMetadata {
    /// Takes a `map` with all ebuild properties, also takes the `md5sum` of the ebuild file.
    ///
    /// Returns `None` if any of the required fields are missing or if the EAPI is invalid.
    pub fn from_map(map: HashMap<&str, &str>, md5sum: String) -> Result<Self> {
        let metadata = Self {
            eapi: map
                .get("EAPI")
                .ok_or_else(|| anyhow!("EAPI not set"))?
                .parse()?,
            description: map
                .get("DESCRIPTION")
                .ok_or_else(|| anyhow!("DESCRIPTION not set"))?
                .to_string(),
            homepage: Self::parse_values(&map, "HOMEPAGE")?,
            src_uri: Self::parse_values(&map, "SRC_URI")?,
            license: Self::parse_values(&map, "LICENSE")?,
            keywords: Self::parse_values(&map, "KEYWORDS")?,
            inherit: Self::parse_values(&map, "INHERIT")?,
            eclasses: Vec::new(), // TODO: parse eclasses
            restrict: Self::parse_expression("RESTRICT", &map)?,
            defined_phases: Self::parse_values(&map, "DEFINED_PHASES")?,
            iuse: Self::parse_values(&map, "IUSE")?,
            required_use: Self::parse_expression("REQUIRED_USE", &map)?,
            slot: map
                .get("SLOT")
                .map(|s| PackageSlot::from_str(s))
                .ok_or_else(|| anyhow!("SLOT not set"))??,
            depend: Self::parse_expression("DEPEND", &map)?,
            bdepend: Self::parse_expression("BDEPEND", &map)?,
            idepend: Self::parse_expression("IDEPEND", &map)?,
            pdepend: Self::parse_expression("PDEPEND", &map)?,
            rdepend: Self::parse_expression("RDEPEND", &map)?,
            md5sum,
        };
        Ok(metadata)
    }

    fn parse_expression<T>(key: &str, map: &HashMap<&str, &str>) -> Result<DepExpression<T>>
    where
        T: ExpressionItem,
    {
        let value = map.get(key).ok_or_else(|| anyhow!("{key} is not set"))?;
        DepExpression::parse(value)
    }

    fn parse_values(map: &HashMap<&str, &str>, key: &str) -> Result<Vec<String>> {
        let parts = map
            .get(key)
            .ok_or_else(|| anyhow!("{key} is missing"))?
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>();
        Ok(parts)
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
        writeln!(f, "IUSE={}", self.iuse.join(" "))?;
        writeln!(f, "KEYWORDS={}", self.keywords.join(" "))?;
        writeln!(f, "LICENSE={}", self.license.join(" "))?;
        writeln!(f, "PDEPEND={}", self.pdepend)?;
        writeln!(f, "RDEPEND={}", self.rdepend)?;
        writeln!(f, "RESTRICT={}", self.restrict)?;
        writeln!(f, "REQUIRED_USE={}", self.required_use)?;
        writeln!(f, "SLOT={}", self.slot)?;
        writeln!(f, "SRC_URI={}", self.src_uri.join(" "))?;
        writeln!(f, "_eclasses_=TODO")?;
        writeln!(f, "_md5_={}", self.md5sum)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_from_map_ok() {
        let data = [
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
            "IUSE=examples ipv6",
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
        .collect::<HashMap<_, _>>();

        let metadata = PackageMetadata::from_map(data, String::new());
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
        assert_eq!(metadata.iuse, vec!["examples", "ipv6"]);
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
}
