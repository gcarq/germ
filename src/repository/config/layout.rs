use anyhow::{Context, Result, anyhow};
use ini::Ini;
use log::debug;
use std::path::Path;

/// Holds the layout configuration of a repository,
/// which defines how the repository is structured and how it resolves ebuilds,
/// eclasses and profiles from parent repositories (masters).
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
        debug!("Loading layout.conf from {}", location.display());
        let conf = Ini::load_from_file(location)?;
        let properties = conf
            .section(None::<String>)
            .with_context(|| anyhow!("cannot parse {}", location.display()))?;

        let name = properties.get("name").map(|s| s.to_owned());
        let masters = properties
            .get("masters")
            .map(|s| {
                s.split_ascii_whitespace()
                    .map(|master| master.to_owned())
                    .collect()
            })
            .ok_or_else(|| anyhow!("missing 'masters' property"))?;
        Ok(Layout { name, masters })
    }
}
