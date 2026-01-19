use anyhow::{Result, anyhow};

/// Represents a profile description as found in profiles.desc file.
#[derive(Debug)]
pub struct ProfileDescription {
    pub keyword: String,
    pub profile_path: String,
    pub stability: String,
}

impl ProfileDescription {
    /// Parses a profile description from a single line.
    /// The line must consist of <keyword> <profile_path> <stability> otherwise an Err is returned.
    pub fn from_line(line: &str) -> Result<Self> {
        let parts = line.split_ascii_whitespace().collect::<Vec<_>>();
        if parts.len() != 3 {
            return Err(anyhow!("Invalid profile description line: {line}"));
        }
        Ok(Self {
            keyword: parts[0].to_owned(),
            profile_path: parts[1].to_owned(),
            stability: parts[2].to_owned(),
        })
    }
}
