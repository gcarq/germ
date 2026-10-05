use std::fmt;
use std::str::FromStr;

use rkyv::{Archive, Deserialize, Serialize};

use crate::Arch;

/// Represents a configured keyword status of a package for a given architecture.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Keyword {
    Stable(Arch),        // arch
    Testing(Arch),       // ~arch
    Unsupported(Arch),   // -arch
    UnlistedUnsupported, // -*, indicates not worth trying to test on unlisted archs.
}

impl Keyword {
    /// Returns the associated arch, if any.
    pub const fn arch(&self) -> Option<&Arch> {
        match self {
            Keyword::Stable(arch) | Keyword::Testing(arch) | Keyword::Unsupported(arch) => {
                Some(arch)
            }
            Keyword::UnlistedUnsupported => None,
        }
    }
}

impl FromStr for Keyword {
    type Err = anyhow::Error;

    fn from_str(keyword: &str) -> Result<Self, Self::Err> {
        let keyword = match keyword.chars().next().unwrap_or_default() {
            '~' => Keyword::Testing(keyword[1..].parse()?),
            '-' => match keyword {
                "-*" => Keyword::UnlistedUnsupported,
                _ => Keyword::Unsupported(keyword[1..].parse()?),
            },
            _ => Keyword::Stable(keyword.parse()?),
        };
        Ok(keyword)
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
            ("amd64", Keyword::Stable("amd64".parse().unwrap())),
            ("~arm64", Keyword::Testing("arm64".parse().unwrap())),
            ("-x86", Keyword::Unsupported("x86".parse().unwrap())),
            ("-*", Keyword::UnlistedUnsupported),
        ];
        for (input, expected) in tests {
            assert_eq!(Keyword::from_str(input).unwrap(), expected);
        }
    }

    #[test]
    fn test_keyword_invalid() {
        for input in ["", "!", "~", "-", "~~amd64", "-*x86"] {
            assert!(Keyword::from_str(input).is_err());
        }
    }
}
