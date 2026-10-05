use std::fs;
use std::path::Path;

use anyhow::{Context, anyhow};
use germ_pms::Eapi;

/// Creates a new instance from the EAPI file at the given `path`.
///
/// Returns `Eapi::default()` if no eapi file exists.
pub fn read_eapi(path: &Path) -> anyhow::Result<Eapi> {
    if !path.exists() {
        return Ok(Eapi::default());
    }
    Ok(fs::read_to_string(path)
        .with_context(|| format!("unable to read eapi file {}", path.display()))?
        .lines()
        .next()
        .ok_or_else(|| anyhow!("empty eapi file {}", path.display()))?
        .parse()?)
}

/// Returns `true` if this EAPI is supported for ebuilds.
pub const fn is_supported_for_ebuilds(eapi: &Eapi) -> bool {
    matches!(eapi, Eapi::Seven | Eapi::Eight | Eapi::Nine)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn test_is_supported_for_ebuilds() {
        let test_cases = [
            (Eapi::Zero, false),
            (Eapi::One, false),
            (Eapi::Two, false),
            (Eapi::Three, false),
            (Eapi::Four, false),
            (Eapi::Five, false),
            (Eapi::Six, false),
            (Eapi::Seven, true),
            (Eapi::Eight, true),
            (Eapi::Nine, true),
        ];
        for (eapi, exp_supported) in test_cases {
            assert_eq!(is_supported_for_ebuilds(&eapi), exp_supported);
        }
    }

    #[test]
    fn test_read_eapi() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("eapi");

        assert_eq!(read_eapi(&path)?, Eapi::default());

        fs::write(&path, "9\n8\n")?;
        assert_eq!(read_eapi(&path)?, Eapi::Nine);

        fs::write(&path, "")?;
        assert!(read_eapi(&path).is_err());

        fs::write(&path, " 8\n")?;
        assert!(read_eapi(&path).is_err());
        Ok(())
    }
}
