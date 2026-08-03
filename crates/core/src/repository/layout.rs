use anyhow::{Context, Result, anyhow};
use ini::Ini;
use log::warn;
use std::path::Path;

/// Holds all supported profile formats and their capabilities.
/// See `man portage 5` and https://www.gentoo.org/glep/glep-0082.html
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProfileFormat {
    Pms,
    Portage1,
    Portage2,
}

impl TryFrom<&str> for ProfileFormat {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "pms" => Ok(Self::Pms),
            "portage-1" => Ok(Self::Portage1),
            "portage-2" => Ok(Self::Portage2),
            _ => Err(value.to_owned()),
        }
    }
}

/// Holds the layout configuration of a [`Repository`],
/// which defines how the repository is structured and how it resolves ebuilds,
/// eclasses and profiles from parent repositories (masters).
#[derive(Debug)]
pub struct Layout {
    // Allows overriding `profiles/repo_name`, although discouraged
    pub name: Option<String>,
    // Defines parent repositories to resolve ebuilds, eclasses and profiles from
    pub masters: Vec<String>,
    profile_formats: Vec<ProfileFormat>,
}

impl Layout {
    /// Builds a [`Layout`] from the given `location`.
    /// Return Err if the file doesn't exist, is not a valid INI file or if
    /// required properties are missing.
    pub fn from_path(location: &Path) -> Result<Self> {
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
        let profile_formats =
            Self::parse_profile_formats(properties.get("profile-formats").unwrap_or_default());

        let layout = Layout {
            name,
            masters,
            profile_formats,
        };
        Ok(layout)
    }

    /// Returns whether mask and USE configuration entries may be directories.
    pub fn supports_profile_file_dirs(&self) -> bool {
        self.profile_formats
            .iter()
            .any(|format| matches!(format, ProfileFormat::Portage1 | ProfileFormat::Portage2))
    }

    /// Returns whether profile parents may refer to another repository.
    pub fn supports_cross_repo_parents(&self) -> bool {
        self.profile_formats.contains(&ProfileFormat::Portage2)
    }

    /// Returns whether profile parents may be relative to the repository profiles root.
    pub fn supports_root_relative_parents(&self) -> bool {
        self.profile_formats.contains(&ProfileFormat::Portage2)
    }

    /// Parses the `profile-formats` property into a list of [`ProfileFormat`].
    fn parse_profile_formats(value: &str) -> Vec<ProfileFormat> {
        let mut profile_formats = Vec::new();
        for format in value.split_ascii_whitespace() {
            match ProfileFormat::try_from(format) {
                Ok(format) if !profile_formats.contains(&format) => profile_formats.push(format),
                Ok(_) => {}
                Err(format) => {
                    warn!("Unknown repository profile format: {format}");
                }
            }
        }
        if profile_formats.is_empty() {
            profile_formats.push(ProfileFormat::Pms);
        }
        profile_formats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_layout(profile_formats: Option<&str>) -> Layout {
        let profile_formats = profile_formats
            .map(|value| format!("profile-formats = {value}\n"))
            .unwrap_or_default();
        let conf = Ini::load_from_str(&format!(
            "name = local\nmasters = kde gentoo\n{profile_formats}"
        ))
        .unwrap();
        Layout::from_ini(&conf).unwrap()
    }

    #[test]
    fn test_layout_from_ini_ok() {
        let layout = parse_layout(None);
        assert_eq!(layout.name, Some("local".to_owned()));
        assert_eq!(layout.masters, vec!["kde".to_owned(), "gentoo".to_owned()]);
    }

    #[test]
    fn test_profile_formats_default() {
        let layout = parse_layout(None);
        assert_eq!(layout.profile_formats, vec![ProfileFormat::Pms]);
    }

    #[test]
    fn test_pms_capabilities() {
        let layout = parse_layout(Some("pms"));
        assert!(!layout.supports_profile_file_dirs());
        assert!(!layout.supports_cross_repo_parents());
        assert!(!layout.supports_root_relative_parents());
    }

    #[test]
    fn test_portage1_capabilities() {
        let layout = parse_layout(Some("portage-1"));
        assert!(layout.supports_profile_file_dirs());
        assert!(!layout.supports_cross_repo_parents());
        assert!(!layout.supports_root_relative_parents());
    }

    #[test]
    fn test_portage2_capabilities() {
        let layout = parse_layout(Some("portage-2"));
        assert!(layout.supports_profile_file_dirs());
        assert!(layout.supports_cross_repo_parents());
        assert!(layout.supports_root_relative_parents());
    }

    #[test]
    fn test_additive_profile_formats() {
        let layout = parse_layout(Some("pms portage-1 portage-2"));
        assert_eq!(
            layout.profile_formats,
            vec![
                ProfileFormat::Pms,
                ProfileFormat::Portage1,
                ProfileFormat::Portage2
            ]
        );
        assert!(layout.supports_profile_file_dirs());
        assert!(layout.supports_cross_repo_parents());
    }

    #[test]
    fn test_unknown_profile_formats() {
        let mixed = parse_layout(Some("future-format portage-1"));
        assert_eq!(mixed.profile_formats, vec![ProfileFormat::Portage1]);

        let unknown = parse_layout(Some("future-format"));
        assert_eq!(unknown.profile_formats, vec![ProfileFormat::Pms]);
    }
}
