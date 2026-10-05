use std::str::FromStr;

use germ_pms::{Arch, Keyword};

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
    fn test_keyword_selector_match() -> anyhow::Result<()> {
        assert!(KeywordSelector::from_str("amd64")?.matches(Some(&"amd64".parse()?)));
        assert!(KeywordSelector::from_str("~*")?.matches(Some(&"~arm64".parse()?)));
        assert!(!KeywordSelector::from_str("*")?.matches(Some(&"~amd64".parse()?)));
        assert!(!KeywordSelector::from_str("~*")?.matches(Some(&"-amd64".parse()?)));
        assert!(KeywordSelector::from_str("**")?.matches(Some(&"-*".parse()?)));
        assert!(KeywordSelector::from_str("**")?.matches(None));
        Ok(())
    }
}
