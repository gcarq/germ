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
    /// Checks if the given metadata keyword matches this selector,
    /// interpreting stable keywords according to [`KeywordView].
    pub fn matches(&self, keyword: Option<&Keyword>, view: KeywordView) -> bool {
        match (self, keyword) {
            (Self::Stable(expected), Some(Keyword::Stable(actual))) => {
                view == KeywordView::AsIs && expected == actual
            }
            (Self::Testing(expected), Some(Keyword::Testing(actual))) => expected == actual,
            (Self::Testing(expected), Some(Keyword::Stable(actual))) => {
                view == KeywordView::StableAsTesting && expected == actual
            }
            (Self::AnyStable, Some(Keyword::Stable(_))) => view == KeywordView::AsIs,
            (Self::AnyTesting, Some(Keyword::Testing(_))) => true,
            (Self::AnyTesting, Some(Keyword::Stable(_))) => view == KeywordView::StableAsTesting,
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

/// Determines how a [`Keyword`] is interpreted when matched against a [`KeywordSelector`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeywordView {
    /// The keyword is interpreted as written.
    AsIs,
    /// Stable keywords are interpreted as their testing counterparts.
    StableAsTesting,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_selector_match() -> anyhow::Result<()> {
        let cases = [
            ("amd64", Some("amd64"), KeywordView::AsIs, true),
            ("~amd64", Some("~amd64"), KeywordView::AsIs, true),
            ("~amd64", Some("amd64"), KeywordView::AsIs, false),
            ("*", Some("amd64"), KeywordView::AsIs, true),
            ("*", Some("~amd64"), KeywordView::AsIs, false),
            ("~*", Some("~amd64"), KeywordView::AsIs, true),
            ("~*", Some("amd64"), KeywordView::AsIs, false),
            ("~*", Some("-amd64"), KeywordView::AsIs, false),
            ("**", None, KeywordView::AsIs, true),
            ("**", Some("-amd64"), KeywordView::AsIs, true),
            ("amd64", Some("amd64"), KeywordView::StableAsTesting, false),
            ("~amd64", Some("amd64"), KeywordView::StableAsTesting, true),
            ("*", Some("amd64"), KeywordView::StableAsTesting, false),
            ("~*", Some("amd64"), KeywordView::StableAsTesting, true),
        ];

        for (selector, keyword, view, expected) in cases {
            let keyword = keyword.map(str::parse::<Keyword>).transpose()?;
            let result = KeywordSelector::from_str(selector)?.matches(keyword.as_ref(), view);
            assert_eq!(result, expected);
        }
        Ok(())
    }
}
