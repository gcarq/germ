use std::{fmt, str::FromStr};

use rkyv::{Archive, Deserialize, Serialize};

use crate::repository::Arch;

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

/// Represents a configured keyword selector, used to match against package keywords.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeywordSelector {
    Stable(Arch),  // arch
    Testing(Arch), // ~arch
    AnyStable,     // *
    AnyTesting,    // ~*
    Any,           // **, also matches empty keywords
}

impl KeywordSelector {
    /// Checks if the given metadata keyword matches this selector.
    pub fn matches(&self, keyword: Option<&Keyword>) -> bool {
        match (self, keyword) {
            (Self::Stable(expected), Some(Keyword::Stable(actual))) => expected == actual,
            (Self::Testing(expected), Some(Keyword::Testing(actual))) => expected == actual,
            (Self::AnyStable, Some(Keyword::Stable(_))) => true,
            (Self::AnyTesting, Some(Keyword::Testing(_))) => true,
            (Self::Any, _) => true,
            _ => false,
        }
    }
}

impl FromStr for KeywordSelector {
    type Err = anyhow::Error;

    fn from_str(selector: &str) -> Result<Self, Self::Err> {
        match selector {
            "*" => Ok(Self::AnyStable),
            "~*" => Ok(Self::AnyTesting),
            "**" => Ok(Self::Any),
            _ => match selector.strip_prefix('~') {
                Some(arch) => Ok(Self::Testing(arch.parse()?)),
                None => Ok(Self::Stable(selector.parse()?)),
            },
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

    #[test]
    fn test_keyword_selector_match() -> anyhow::Result<()> {
        assert!(KeywordSelector::from_str("amd64")?.matches(Some(&Keyword::from_str("amd64")?)));
        assert!(KeywordSelector::from_str("~*")?.matches(Some(&Keyword::from_str("~arm64")?)));
        assert!(!KeywordSelector::from_str("*")?.matches(Some(&Keyword::from_str("~amd64")?)));
        assert!(!KeywordSelector::from_str("~*")?.matches(Some(&Keyword::from_str("-amd64")?)));
        assert!(KeywordSelector::from_str("**")?.matches(Some(&Keyword::UnlistedUnsupported)));
        assert!(KeywordSelector::from_str("**")?.matches(None));
        Ok(())
    }
}
