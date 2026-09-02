use std::{fmt, str::FromStr};

use rkyv::{Archive, Deserialize, Serialize};

use crate::repository::Arch;

/// Represents the keyword status of a package for a given architecture.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Keyword {
    Stable(Arch),        // arch
    Testing(Arch),       // ~arch
    Unsupported(Arch),   // -arch
    UnlistedUnsupported, // -*, indicates not worth trying to test on unlisted archs.
}

impl Keyword {
    pub fn new(keyword: &str) -> anyhow::Result<Self> {
        let keyword = match keyword.chars().next().unwrap_or_default() {
            '~' => Keyword::Testing(Arch::new(&keyword[1..])?),
            '-' => match keyword {
                "-*" => Keyword::UnlistedUnsupported,
                _ => Keyword::Unsupported(Arch::new(&keyword[1..])?),
            },
            _ => Keyword::Stable(Arch::new(keyword)?),
        };
        Ok(keyword)
    }

    /// Returns the associated arch, if any.
    pub const fn arch(&self) -> Option<&Arch> {
        match self {
            Keyword::Stable(arch) => Some(arch),
            Keyword::Testing(arch) => Some(arch),
            Keyword::Unsupported(arch) => Some(arch),
            Keyword::UnlistedUnsupported => None,
        }
    }
}

impl FromStr for Keyword {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl fmt::Display for Keyword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Keyword::Stable(arch) => write!(f, "{arch}"),
            Keyword::Testing(arch) => write!(f, "~{arch}"),
            Keyword::Unsupported(arch) => write!(f, "-{arch}"),
            Keyword::UnlistedUnsupported => write!(f, "-*"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_valid() {
        let tests = [
            ("amd64", Keyword::Stable(Arch::new("amd64").unwrap())),
            ("~arm64", Keyword::Testing(Arch::new("arm64").unwrap())),
            ("-x86", Keyword::Unsupported(Arch::new("x86").unwrap())),
            ("-*", Keyword::UnlistedUnsupported),
        ];
        for (input, expected) in tests {
            let parsed = Keyword::new(input).unwrap();
            assert_eq!(parsed.arch(), expected.arch());
        }
    }

    #[test]
    fn test_keyword_invalid() {
        let tests = ["", "!", "~", "-", "~~amd64", "-*x86"];
        for input in tests {
            assert!(Keyword::new(input).is_err());
        }
    }
}
