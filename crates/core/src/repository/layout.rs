use anyhow::{Context, Result, anyhow};
use ini::Ini;
use log::debug;
use std::path::Path;

/// Holds the layout configuration of a [`Repository`],
/// which defines how the repository is structured and how it resolves ebuilds,
/// eclasses and profiles from parent repositories (masters).
#[cfg_attr(test, derive(Debug))]
pub struct Layout {
    // Allows overriding `profiles/repo_name`, although discouraged
    pub name: Option<String>,
    // Defines parent repositories to resolve ebuilds, eclasses and profiles from
    pub masters: Vec<String>,
}

impl Layout {
    /// Builds a [`Layout`] from the given `location`.
    /// Return Err if the file doesn't exist, is not a valid INI file or if
    /// required properties are missing.
    pub fn from_path(location: &Path) -> Result<Self> {
        debug!("Loading layout.conf from {} ...", location.display());
        let conf = Ini::load_from_file(location)?;
        Self::from_ini(&conf).with_context(|| anyhow!("cannot parse {}", location.display()))
    }

    fn from_ini(conf: &Ini) -> Result<Self> {
        let properties = conf
            .section(None::<String>)
            .with_context(|| "no global properties defined")?;

        let name = properties.get("name").map(ToOwned::to_owned);
        let masters = properties
            .get("masters")
            .map(|s| s.split_ascii_whitespace().map(ToOwned::to_owned).collect())
            .ok_or_else(|| anyhow!("missing 'masters' property"))?;
        Ok(Layout { name, masters })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_from_ini_ok() {
        let ini_str = r#"
            name = local
            masters = kde gentoo
        "#;
        let conf = Ini::load_from_str(ini_str).unwrap();
        let layout = Layout::from_ini(&conf).unwrap();
        assert_eq!(layout.name, Some("local".to_owned()));
        assert_eq!(layout.masters, vec!["kde".to_owned(), "gentoo".to_owned()]);
    }
}
