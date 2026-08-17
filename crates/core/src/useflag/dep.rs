use super::UseFlag;
use anyhow::bail;
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Represents the kind of an atom USE dependency, see PMS 8.3.4 for more information.
#[derive(
    Archive, Serialize, Deserialize, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Debug,
)]
pub enum UseDepKind {
    Enabled,             // foo
    Disabled,            // -foo
    ConditionalEnabled,  // foo?
    ConditionalDisabled, // !foo?
    Equal,               //foo=
    NotEqual,            // !foo=
}

impl UseDepKind {
    const fn prefix(self) -> &'static str {
        match self {
            Self::Enabled | Self::ConditionalEnabled | Self::Equal => "",
            Self::Disabled => "-",
            Self::ConditionalDisabled | Self::NotEqual => "!",
        }
    }

    const fn suffix(self) -> &'static str {
        match self {
            Self::Enabled | Self::Disabled => "",
            Self::ConditionalEnabled | Self::ConditionalDisabled => "?",
            Self::Equal | Self::NotEqual => "=",
        }
    }
}

/// Represents the default state of an atom USE dependency.
#[derive(
    Archive, Serialize, Deserialize, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Debug,
)]
pub enum UseDepDefault {
    Enabled,  // foo(+)
    Disabled, // foo(-)
}

impl fmt::Display for UseDepDefault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Enabled => "(+)",
            Self::Disabled => "(-)",
        })
    }
}

/// Represents an atom USE dependency, see PMS 8.3.4 for more information.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct UseDep {
    flag: UseFlag,
    kind: UseDepKind,
    default: Option<UseDepDefault>,
}

impl FromStr for UseDep {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> anyhow::Result<Self> {
        let (prefix, rest) = match input.as_bytes().first().copied() {
            Some(prefix @ (b'!' | b'-')) => (Some(prefix as char), &input[1..]),
            _ => (None, input),
        };
        let (suffix, rest) = match rest.as_bytes().last().copied() {
            Some(suffix @ (b'?' | b'=')) => (Some(suffix as char), &rest[..rest.len() - 1]),
            _ => (None, rest),
        };
        let (default, flag) = if let Some(flag) = rest.strip_suffix("(+)") {
            (Some(UseDepDefault::Enabled), flag)
        } else if let Some(flag) = rest.strip_suffix("(-)") {
            (Some(UseDepDefault::Disabled), flag)
        } else {
            (None, rest)
        };

        let flag = flag.parse()?;
        let kind = match (prefix, suffix) {
            (None, None) => UseDepKind::Enabled,
            (Some('-'), None) => UseDepKind::Disabled,
            (None, Some('?')) => UseDepKind::ConditionalEnabled,
            (Some('!'), Some('?')) => UseDepKind::ConditionalDisabled,
            (None, Some('=')) => UseDepKind::Equal,
            (Some('!'), Some('=')) => UseDepKind::NotEqual,
            _ => bail!("invalid USE dependency: '{input}'"),
        };

        Ok(Self {
            flag,
            kind,
            default,
        })
    }
}

impl fmt::Display for UseDep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind.prefix())?;
        self.flag.fmt(f)?;
        if let Some(default) = self.default {
            default.fmt(f)?;
        }
        f.write_str(self.kind.suffix())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_use_dep_valid() {
        let inputs = [
            "foo", "-foo", "foo?", "!foo?", "foo=", "!foo=", "foo(+)", "foo(-)", "-foo(+)",
            "-foo(-)", "foo(+)?", "foo(-)?", "!foo(+)?", "!foo(-)?", "foo(+)=", "foo(-)=",
            "!foo(+)=", "!foo(-)=",
        ];

        for input in inputs {
            let dependency = input.parse::<UseDep>().unwrap();
            assert_eq!(dependency.flag, "foo".parse().unwrap());
            assert_eq!(dependency.to_string(), input);
        }
    }

    #[test]
    fn test_use_dep_structure() {
        let dependency = "!foo(-)?".parse::<UseDep>().unwrap();
        assert_eq!(dependency.flag, "foo".parse().unwrap());
        assert_eq!(dependency.kind, UseDepKind::ConditionalDisabled);
        assert_eq!(dependency.default, Some(UseDepDefault::Disabled));
    }

    #[test]
    fn test_use_dep_invalid() {
        for input in [
            "+foo",
            "!foo",
            "-foo?",
            "-foo=",
            "foo!",
            "foo??",
            "foo==",
            "foo(+)(-)",
            "foo?(+)",
            "foo=(+)",
            "(+)",
            "(-)",
            "foo()",
            "foo(+x)",
            "foo bar",
        ] {
            assert!(
                input.parse::<UseDep>().is_err(),
                "{input:?} should be invalid"
            );
        }
    }
}
