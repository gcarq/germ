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
            .filter_map(|(atom, flags)| {
                let flags = flags.into_values().collect::<FxHashSet<_>>();
                if flags.is_empty() {
                    None
                } else {
                    Some((atom, flags))
                }
            })
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
        for (atom, parent_flags) in &parent.0 {
            if let Some(child_flags) = self.0.get_mut(atom) {
                child_flags.inherit_from(parent_flags);
            } else {
                self.0.insert(atom.clone(), parent_flags.clone());
            }
        }
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
            if seen.insert(flag.inner().clone()) {
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
    use crate::files::entry::Prefixed::{Set, Unset};
    use std::str::FromStr;

    #[test]
    fn test_pkg_use_entries_from_string() -> Result<()> {
        let content = "
            # Enable sysvinit symlinks by default.
            sys-apps/systemd sysv-utils

            app-admin/sudo foo -bar baz
            app-admin/sudo foo
        ";

        let file = PackageUseEntries::from_string(content.into())?;
        let expected = vec![
            (
                Atom::new("sys-apps/systemd")?,
                PrefixedUseFlags(vec![Set(UseFlag::from_str("sysv-utils")?)]),
            ),
            (
                Atom::new("app-admin/sudo")?,
                PrefixedUseFlags(vec![
                    Unset(UseFlag::from_str("bar")?),
                    Set(UseFlag::from_str("baz")?),
                    Set(UseFlag::from_str("foo")?),
                ]),
            ),
        ]
        .into_iter()
        .collect();
        assert_eq!(file.0, expected);
        Ok(())
    }

    #[test]
    fn test_pkg_use_entries_inherit_from() -> Result<()> {
        let grand_parent = PackageUseEntries::from_string(
            "
            dev-libs/libffi foo -bar baz foobar
            app-arch/xz-utils foo bar -test
            app-arch/zstd baz -foo
            sys-apps/systemd -foo
            dev-libs/libffi foobar
            "
            .into(),
        )?;

        let parent = PackageUseEntries::from_string(
            "
            dev-libs/libffi foobar
            app-arch/xz-utils -foo bar baz test
            app-arch/rpm -foo
            "
            .into(),
        )?;

        let mut child = PackageUseEntries::from_string(
            "
            app-arch/xz-utils -foo -bar baz
            app-arch/zstd foo
            app-arch/rpm foo
            dev-libs/libffi -foobar
            "
            .into(),
        )?;
        child.inherit_from(&parent.inherit(&grand_parent));

        assert_eq!(child.0.len(), 5);
        assert_eq!(
            child.0.get(&Atom::new("dev-libs/libffi")?).unwrap(),
            &PrefixedUseFlags(vec![
                Unset(UseFlag::from_str("foobar")?),
                Set(UseFlag::from_str("foo")?),
                Unset(UseFlag::from_str("bar")?),
                Set(UseFlag::from_str("baz")?),
            ])
        );
        assert_eq!(
            child.0.get(&Atom::new("app-arch/zstd")?).unwrap(),
            &PrefixedUseFlags(vec![
                Set(UseFlag::from_str("foo")?),
                Set(UseFlag::from_str("baz")?),
            ])
        );
        assert_eq!(
            child.0.get(&Atom::new("app-arch/rpm")?).unwrap(),
            &PrefixedUseFlags(vec![Set(UseFlag::from_str("foo")?)])
        );
        assert_eq!(
            child.0.get(&Atom::new("app-arch/xz-utils")?).unwrap(),
            &PrefixedUseFlags(vec![
                Set(UseFlag::from_str("baz")?),
                Unset(UseFlag::from_str("bar")?),
                Unset(UseFlag::from_str("foo")?),
                Set(UseFlag::from_str("test")?),
            ])
        );
        Ok(())
    }

    #[test]
    fn test_pkg_use_entries_finalize() -> Result<()> {
        let entries = PackageUseEntries::from_string("dev-libs/libffi foo -bar baz".into())?;
        let finalized = entries.finalize();
        let expected = vec![(
            Atom::new("dev-libs/libffi")?,
            vec![UseFlag::from_str("foo")?, UseFlag::from_str("baz")?]
                .into_iter()
                .collect(),
        )]
        .into_iter()
        .collect();
        assert_eq!(finalized, expected);
        Ok(())
    }

    #[test]
    fn test_prefixed_use_flags_inherit_from() -> Result<()> {
        let parent = PrefixedUseFlags(vec![
            Set(UseFlag::from_str("foo")?),
            Unset(UseFlag::from_str("qux")?),
            Set(UseFlag::from_str("baz")?),
        ]);

        let mut child = PrefixedUseFlags(vec![
            Unset(UseFlag::from_str("foo")?),
            Set(UseFlag::from_str("qux")?),
        ]);

        child.inherit_from(&parent);

        let expected = PrefixedUseFlags(vec![
            Set(UseFlag::from_str("qux")?),
            Unset(UseFlag::from_str("foo")?),
            Set(UseFlag::from_str("baz")?),
        ]);
        assert_eq!(child, expected);
        Ok(())
    }
}
