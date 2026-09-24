use super::{EffectiveUse, UseFlag};
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

impl UseDepDefault {
    /// Returns whether this default state is enabled.
    pub const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
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

impl UseDep {
    /// Returns whether this dependency is satisfied by the `owner` and `target` package USE states.
    ///
    /// TODO: This currently dosn't check whether a USE flag is available for `target`.
    pub fn is_satisfied_by(
        &self,
        owner: &EffectiveUse,
        target: &EffectiveUse,
    ) -> anyhow::Result<bool> {
        let target = target
            .state(&self.flag)
            .or_else(|| self.default.map(UseDepDefault::is_enabled));

        let matches = match self.kind {
            UseDepKind::Enabled => target == Some(true),
            UseDepKind::Disabled => target == Some(false),
            UseDepKind::ConditionalEnabled => {
                target == Some(true) || !owner.is_enabled(&self.flag)?
            }
            UseDepKind::ConditionalDisabled => {
                target == Some(false) || owner.is_enabled(&self.flag)?
            }
            UseDepKind::Equal => target == Some(owner.state(&self.flag).unwrap_or(false)),
            UseDepKind::NotEqual => target == Some(!owner.state(&self.flag).unwrap_or(false)),
        };
        Ok(matches)
    }
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
    use crate::useflag::test_support::effective;

    type EffectiveArgs<'a> = (&'a [&'a str], &'a [&'a str]);

    #[test]
    fn test_use_dep_matching() {
        // input, (owner_available, owner_enabled), (target_available, target_enabled), expected
        let cases: [(&str, EffectiveArgs<'_>, EffectiveArgs<'_>, bool); 24] = [
            ("foo", (&["foo"], &["foo"]), (&["foo"], &["foo"]), true),
            ("foo", (&["foo"], &["foo"]), (&["foo"], &[]), false),
            ("foo", (&["foo"], &["foo"]), (&[], &[]), false),
            ("-foo", (&["foo"], &["foo"]), (&["foo"], &[]), true),
            ("-foo", (&["foo"], &["foo"]), (&["foo"], &["foo"]), false),
            ("-foo", (&["foo"], &["foo"]), (&[], &[]), false),
            ("foo?", (&["foo"], &["foo"]), (&["foo"], &["foo"]), true),
            ("foo?", (&["foo"], &["foo"]), (&["foo"], &[]), false),
            ("foo?", (&["foo"], &[]), (&[], &[]), true),
            ("!foo?", (&["foo"], &[]), (&["foo"], &[]), true),
            ("!foo?", (&["foo"], &[]), (&["foo"], &["foo"]), false),
            ("!foo?", (&["foo"], &["foo"]), (&[], &[]), true),
            ("foo=", (&["foo"], &["foo"]), (&["foo"], &["foo"]), true),
            ("foo=", (&["foo"], &[]), (&["foo"], &[]), true),
            ("foo=", (&[], &[]), (&["foo"], &[]), true),
            ("foo=", (&[], &[]), (&["foo"], &["foo"]), false),
            ("!foo=", (&["foo"], &["foo"]), (&["foo"], &[]), true),
            ("!foo=", (&["foo"], &[]), (&["foo"], &["foo"]), true),
            ("!foo=", (&[], &[]), (&["foo"], &["foo"]), true),
            ("!foo=", (&[], &[]), (&["foo"], &[]), false),
            ("foo(+)", (&[], &[]), (&[], &[]), true),
            ("foo(-)", (&[], &[]), (&[], &[]), false),
            ("foo(+)=", (&["foo"], &[]), (&[], &[]), false),
            ("foo(-)=", (&["foo"], &[]), (&[], &[]), true),
        ];

        for (input, owner, target, expected) in cases {
            let dep = input.parse::<UseDep>().unwrap();
            let actual = dep
                .is_satisfied_by(&effective(owner.0, owner.1), &effective(target.0, target.1))
                .unwrap();
            assert_eq!(actual, expected, "{input}");
        }
    }

    #[test]
    fn test_use_dep_conditional_owner() {
        for input in ["foo?", "!foo?"] {
            let dep = input.parse::<UseDep>().unwrap();
            let result = dep.is_satisfied_by(&EffectiveUse::default(), &EffectiveUse::default());
            assert!(result.is_err(), "{input}");
        }
    }

    #[test]
    fn test_use_dep_valid() {
        let inputs = [
            "foo", "-foo", "foo?", "!foo?", "foo=", "!foo=", "foo(+)", "foo(-)", "-foo(+)",
            "-foo(-)", "foo(+)?", "foo(-)?", "!foo(+)?", "!foo(-)?", "foo(+)=", "foo(-)=",
            "!foo(+)=", "!foo(-)=",
        ];
        for input in inputs {
            let dep = input.parse::<UseDep>().unwrap();
            assert_eq!(dep.flag, "foo".parse().unwrap());
            assert_eq!(dep.to_string(), input);
        }

        let dep = "!foo(-)?".parse::<UseDep>().unwrap();
        assert_eq!(dep.kind, UseDepKind::ConditionalDisabled);
        assert_eq!(dep.default, Some(UseDepDefault::Disabled));
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
            let result = input.parse::<UseDep>();
            assert!(result.is_err(), "{input:?} should be invalid");
        }
    }
}
