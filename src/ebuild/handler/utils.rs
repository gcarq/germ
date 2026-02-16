use std::borrow::Cow;

/// Unescapes the given `string` by replacing escape sequences with their corresponding characters.
pub fn unescape(string: &str) -> Cow<'_, str> {
    if !string.contains('\\') {
        return Cow::Borrowed(string);
    }

    let mut out = String::with_capacity(string.len());
    let mut chars = string.chars();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }

    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unescape() {
        assert_eq!(unescape("hello\\nworld"), "hello\nworld");
        assert_eq!(unescape("tab\\tseparated"), "tab\tseparated");
        assert_eq!(unescape("backslash\\\\test"), "backslash\\test");
        assert_eq!(unescape("no\\escapes"), "no\\escapes");
        assert_eq!(unescape("endswithbackslash\\"), "endswithbackslash\\");
    }
}
