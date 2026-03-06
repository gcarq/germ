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
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
pub struct PackageMetadata {
    eapi: Eapi,
    description: String,
    homepage: Vec<String>,
    src_uri: Vec<String>,
    license: Vec<String>,
    keywords: Vec<String>,
    inherit: Vec<String>,
    restrict: String,
    defined_phases: Vec<String>,
    isue: Vec<String>,
    required_use: String,
    slot: PackageSlot,
    depend: String,
    bdepend: String,
    idepend: String,
    pdepend: String,
    rdepend: String,
    eclasses: Vec<Eclass>,
    md5sum: String,
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
            homepage: Self::sanitize_value(&map, "HOMEPAGE")?,
            src_uri: Self::sanitize_value(&map, "SRC_URI")?,
            license: Self::sanitize_value(&map, "LICENSE")?,
            keywords: Self::sanitize_value(&map, "KEYWORDS")?,
            inherit: Self::sanitize_value(&map, "INHERIT")?,
            eclasses: Vec::new(), // TODO: parse eclasses
            restrict: map
                .get("RESTRICT")
                .ok_or_else(|| anyhow!("RESTRICT not set"))?
                .to_string(),
            defined_phases: Self::sanitize_value(&map, "DEFINED_PHASES")?,
            isue: Self::sanitize_value(&map, "IUSE")?,
            required_use: Self::sanitize_value(&map, "REQUIRED_USE")?.join(" "),
            slot: map
                .get("SLOT")
                .map(|s| PackageSlot::from_str(s))
                .ok_or_else(|| anyhow!("SLOT not set"))??,
            depend: Self::sanitize_value(&map, "DEPEND")?.join(" "),
            bdepend: Self::sanitize_value(&map, "BDEPEND")?.join(" "),
            idepend: Self::sanitize_value(&map, "IDEPEND")?.join(" "),
            pdepend: Self::sanitize_value(&map, "PDEPEND")?.join(" "),
            rdepend: Self::sanitize_value(&map, "RDEPEND")?.join(" "),
            md5sum,
        };
        Ok(metadata)
    }

    fn sanitize_value(map: &HashMap<&str, &str>, key: &str) -> Result<Vec<String>> {
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
        writeln!(f, "IUSE={}", self.isue.join(" "))?;
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
            "BDEPEND= \tpython_single_target_python3_11? ( \t dev-python/setuptools[python_targets_python3_11(-)] \t )",
            "EAPI=8",
            "PROPERTIES=",
            "DEFINED_PHASES=",
            "IDEPEND=",
            "INHERIT= bash-completion-r1 eapi9-ver edo linux-info systemd",
        ].iter()
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
        assert_eq!(metadata.restrict, "");
        assert_eq!(metadata.defined_phases.len(), 0);
        assert_eq!(metadata.isue, vec!["examples", "ipv6"]);
        assert_eq!(
            metadata.required_use,
            "^^ ( python_single_target_python3_11 )"
        );
        assert_eq!(metadata.slot, PackageSlot::Simple("0".into()));
        assert_eq!(metadata.depend, "");
        assert_eq!(
            metadata.bdepend,
            "python_single_target_python3_11? ( dev-python/setuptools[python_targets_python3_11(-)] )"
        );
        assert_eq!(metadata.idepend, "");
        assert_eq!(metadata.pdepend, "");
        assert_eq!(
            metadata.rdepend,
            "python_single_target_python3_11? ( dev-lang/python:3.11 )"
        );
    }
}
