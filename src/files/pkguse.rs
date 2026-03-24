use crate::deps::atom::Atom;
use crate::deps::useflag::UseFlag;
use crate::files::FileFromPath;
use crate::files::entry::Prefixed;
use crate::types::{FxHashMap, FxHashSet};
use crate::utils::Inherit;
use anyhow::{Context, Result, anyhow};
use std::str::FromStr;

#[derive(Clone, Default)]
#[cfg_attr(test, derive(Debug))]
pub struct PackageUseEntries(FxHashMap<Atom, PrefixedUseFlags>);

impl PackageUseEntries {
    pub fn finalize(self) -> FxHashMap<Atom, FxHashSet<UseFlag>> {
        self.0
            .into_iter()
            .map(|(atom, flags)| (atom, flags.into_values().collect::<FxHashSet<_>>()))
            .collect()
    }

    fn parse_line(line: &str) -> Result<(Atom, PrefixedUseFlags)> {
        match line.split_once(char::is_whitespace) {
            Some((atom, flags)) => Ok((atom.parse()?, flags.parse()?)),
            None => Err(anyhow!("invalid package.use definition: {line}")),
        }
    }
}

impl FileFromPath for PackageUseEntries {
    /// Creates a new instance from the given `content`.
    /// Lines that are empty or start with `#` are ignored.
    fn from_string(content: String) -> Result<Self>
    where
        Self: Sized,
    {
        let mut map = FxHashMap::default();

        for (lineno, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (atom, flags) = Self::parse_line(line)
                .with_context(|| format!("failed to parse line {}: {line}", lineno + 1))?;

            let entry: &mut PrefixedUseFlags = map.entry(atom).or_default();
            entry.update_from(&flags);
        }
        Ok(Self(map))
    }
}

impl Inherit for PackageUseEntries {
    fn inherit_from(&mut self, parent: &Self) {
        let mut result = self.0.clone();
        for (atom, parent_flags) in &parent.0 {
            if let Some(child_flags) = result.get_mut(atom) {
                child_flags.inherit_from(parent_flags);
            } else {
                let flags = parent_flags.clone().clear_unsets();
                if !flags.0.is_empty() {
                    result.insert(atom.clone(), flags);
                }
            }
        }
        self.0 = result;
    }
}

/// Helper struct to manage unique USE flags with their prefixes for a single package.
#[derive(Eq, PartialEq, Clone, Default)]
#[cfg_attr(test, derive(Debug))]
struct PrefixedUseFlags(Vec<Prefixed<UseFlag>>);

impl PrefixedUseFlags {
    /// Updates the flags with the given `flag`, replacing any existing flag with the same name.
    pub fn update(&mut self, flag: &Prefixed<UseFlag>) {
        self.0.retain(|f| f != flag);
        self.0.push(flag.clone());
    }

    /// Updates the flags with the given `other` flags, replacing any existing flags with the same
    /// name.
    pub fn update_from(&mut self, other: &Self) {
        for flag in &other.0 {
            self.update(flag);
        }
    }

    pub fn clear_unsets(mut self) -> Self {
        self.0.retain(Prefixed::is_set);
        self
    }

    pub fn into_values(self) -> impl Iterator<Item = UseFlag> {
        self.0.into_iter().filter_map(Prefixed::into_value)
    }
}

impl Inherit for PrefixedUseFlags {
    /// Inherits flags from the given `parent`, replacing any existing flags with the same name.
    /// Flags that are unset in the child will not be inherited from the parent.
    fn inherit_from(&mut self, parent: &Self) {
        let mut seen = FxHashSet::default();
        let mut result = Vec::new();

        for flag in self.0.iter().rev().chain(parent.0.iter().rev()) {
            let key = flag.inner();
            if seen.insert(key.clone()) && flag.is_set() {
                result.push(flag.clone());
            }
        }

        self.0 = result;
    }
}

impl FromStr for PrefixedUseFlags {
    type Err = anyhow::Error;

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        let mut flags = PrefixedUseFlags::default();
        for flag in string.split_whitespace().map(str::parse) {
            flags.update(&flag?);
        }
        Ok(flags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_pkg_use_entries_from_string() {
        let content = "
            # Enable sysvinit symlinks by default.
            sys-apps/systemd sysv-utils

            app-admin/sudo foo -bar baz
            app-admin/sudo foo
        ";

        let file = PackageUseEntries::from_string(content.into()).unwrap();
        let expected = vec![
            (
                Atom::new("sys-apps/systemd").unwrap(),
                PrefixedUseFlags(vec![Prefixed::Set(
                    UseFlag::from_str("sysv-utils").unwrap(),
                )]),
            ),
            (
                Atom::new("app-admin/sudo").unwrap(),
                PrefixedUseFlags(vec![
                    Prefixed::Unset(UseFlag::from_str("bar").unwrap()),
                    Prefixed::Set(UseFlag::from_str("baz").unwrap()),
                    Prefixed::Set(UseFlag::from_str("foo").unwrap()),
                ]),
            ),
        ]
        .into_iter()
        .collect();
        assert_eq!(file.0, expected);
    }

    #[test]
    fn test_pkg_use_entries_inherit_from() {
        let parent = PackageUseEntries::from_string(
            "
            dev-libs/libffi foo -bar baz foobar
            app-arch/xz-utils foo bar
            app-arch/zstd baz -foo
            sys-apps/systemd -foo
            dev-libs/libffi foobar
            "
            .into(),
        )
        .unwrap();
        let mut child = PackageUseEntries::from_string(
            "
            app-arch/xz-utils -foo -bar baz
            app-arch/zstd foo
            app-arch/rpm foo
            dev-libs/libffi -foobar
            "
            .into(),
        )
        .unwrap();
        child.inherit_from(&parent);

        assert_eq!(child.0.len(), 4);
        assert_eq!(
            child.0.get(&Atom::new("dev-libs/libffi").unwrap()).unwrap(),
            &PrefixedUseFlags(vec![
                Prefixed::Set(UseFlag::from_str("baz").unwrap()),
                Prefixed::Set(UseFlag::from_str("foo").unwrap()),
            ])
        );
        assert_eq!(
            child.0.get(&Atom::new("app-arch/zstd").unwrap()).unwrap(),
            &PrefixedUseFlags(vec![
                Prefixed::Set(UseFlag::from_str("foo").unwrap()),
                Prefixed::Set(UseFlag::from_str("baz").unwrap()),
            ])
        );
        assert_eq!(
            child.0.get(&Atom::new("app-arch/rpm").unwrap()).unwrap(),
            &PrefixedUseFlags(vec![Prefixed::Set(UseFlag::from_str("foo").unwrap())])
        );
        assert_eq!(
            child
                .0
                .get(&Atom::new("app-arch/xz-utils").unwrap())
                .unwrap(),
            &PrefixedUseFlags(vec![Prefixed::Set(UseFlag::from_str("baz").unwrap())])
        );
    }

    #[test]
    fn test_pkg_use_entries_finalize() {
        let entries =
            PackageUseEntries::from_string("dev-libs/libffi foo -bar baz".into()).unwrap();
        let finalized = entries.finalize();
        let expected = vec![(
            Atom::new("dev-libs/libffi").unwrap(),
            vec![
                UseFlag::from_str("foo").unwrap(),
                UseFlag::from_str("baz").unwrap(),
            ]
            .into_iter()
            .collect(),
        )]
        .into_iter()
        .collect();
        assert_eq!(finalized, expected);
    }

    #[test]
    fn test_prefixed_use_flags_inherit_from() {
        let parent = PrefixedUseFlags(vec![
            Prefixed::Set(UseFlag::from_str("foo").unwrap()),
            Prefixed::Unset(UseFlag::from_str("bar").unwrap()),
            Prefixed::Set(UseFlag::from_str("baz").unwrap()),
        ]);
        let mut child = PrefixedUseFlags(vec![
            Prefixed::Unset(UseFlag::from_str("foo").unwrap()),
            Prefixed::Set(UseFlag::from_str("qux").unwrap()),
        ]);
        child.inherit_from(&parent);

        let expected = PrefixedUseFlags(vec![
            Prefixed::Set(UseFlag::from_str("qux").unwrap()),
            Prefixed::Set(UseFlag::from_str("baz").unwrap()),
        ]);
        assert_eq!(child, expected);
    }
}
