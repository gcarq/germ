use crate::deps::atom::Atom;
use crate::deps::useflag::UseFlag;
use crate::files::content_from_path;
use crate::files::entry::{Entry, Precedence};
use crate::types::{FxHashMap, FxHashSet};
use crate::utils::{Inherit, is_blank_or_comment};
use anyhow::{Context, Result, bail};
use std::path::Path;

#[derive(Clone, Default)]
#[cfg_attr(test, derive(Debug))]
pub struct PackageUseEntries(FxHashMap<Atom, EntryUseFlags>);

impl PackageUseEntries {
    pub fn from_path(path: &Path, order: Precedence, recursive: bool) -> Result<Self> {
        let content = content_from_path(path, recursive, true)?;
        Self::from_string(content, order)
    }

    pub fn from_string(content: String, order: Precedence) -> Result<Self> {
        let mut map = FxHashMap::default();

        for (lineno, line) in content.lines().enumerate() {
            let line = line.trim();
            if is_blank_or_comment(line) {
                continue;
            }
            let (atom, flags) = Self::parse_line(line, order)
                .with_context(|| format!("failed to parse line {}: {line}", lineno + 1))?;

            let entry: &mut EntryUseFlags = map.entry(atom).or_default();
            entry.update_from(&flags);
        }
        Ok(Self(map))
    }

    pub fn into_inner(self) -> FxHashMap<Atom, EntryUseFlags> {
        self.0
    }

    fn parse_line(line: &str, order: Precedence) -> Result<(Atom, EntryUseFlags)> {
        match line.split_once(char::is_whitespace) {
            Some((atom, flags)) => Ok((atom.parse()?, EntryUseFlags::from_str(flags, order)?)),
            None => bail!("invalid package.use definition: {line}"),
        }
    }
}

impl Inherit for PackageUseEntries {
    fn inherit_from(&mut self, parent: &Self) -> anyhow::Result<()> {
        for (atom, parent_flags) in &parent.0 {
            if let Some(child_flags) = self.0.get_mut(atom) {
                child_flags.inherit_from(parent_flags)?;
            } else {
                self.0.insert(atom.clone(), parent_flags.clone());
            }
        }
        Ok(())
    }
}

/// Helper struct to manage unique USE flags with their prefixes for a single package.
#[derive(Eq, PartialEq, Hash, Clone, Default)]
#[cfg_attr(test, derive(Debug))]
pub struct EntryUseFlags(Vec<Entry<UseFlag>>);

impl EntryUseFlags {
    pub fn from_str(value: &str, order: Precedence) -> Result<Self> {
        let mut flags = EntryUseFlags::default();
        for flag in value.split_whitespace().map(|f| Entry::from_str(f, order)) {
            flags.update(&flag?);
        }
        Ok(flags)
    }

    pub const fn from_raw(raw: Vec<Entry<UseFlag>>) -> Self {
        Self(raw)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Entry<UseFlag>> {
        self.0.iter()
    }

    pub fn get_match(&self, flag: &UseFlag) -> Option<&Entry<UseFlag>> {
        self.iter()
            .filter(|f| f.inner() == flag)
            .max_by_key(|f| f.prec)
    }

    /// Updates the flags with the given `flag`, replacing any existing flag with the same name.
    pub fn update(&mut self, flag: &Entry<UseFlag>) {
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
}

impl Inherit for EntryUseFlags {
    /// Inherits flags from the given `parent`, replacing any existing flags with the same name.
    /// Flags that are unset in the child will not be inherited from the parent.
    fn inherit_from(&mut self, parent: &Self) -> anyhow::Result<()> {
        let mut seen = FxHashSet::default();
        let mut result = Vec::new();
        for flag in self.0.iter().rev().chain(parent.0.iter().rev()) {
            if seen.insert(flag.inner()) {
                result.push(flag.clone());
            }
        }
        result.reverse();
        self.0 = result;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkg_use_entries_from_string() -> Result<()> {
        let content = "
            # Enable sysvinit symlinks by default.
            sys-apps/systemd sysv-utils

            app-admin/sudo foo -bar baz
            app-admin/sudo foo
        ";

        let file = PackageUseEntries::from_string(content.into(), Precedence::User)?;
        let expected = vec![
            (
                Atom::new("sys-apps/systemd")?,
                EntryUseFlags(vec![Entry::from_str("sysv-utils", Precedence::User)?]),
            ),
            (
                Atom::new("app-admin/sudo")?,
                EntryUseFlags(vec![
                    Entry::from_str("-bar", Precedence::User)?,
                    Entry::from_str("baz", Precedence::User)?,
                    Entry::from_str("foo", Precedence::User)?,
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
            Precedence::Profile(0),
        )?;

        let parent = PackageUseEntries::from_string(
            "
            dev-libs/libffi foobar
            app-arch/xz-utils -foo bar baz test
            app-arch/rpm -foo
            "
            .into(),
            Precedence::Profile(1),
        )?;

        let mut child = PackageUseEntries::from_string(
            "
            app-arch/xz-utils -foo -bar baz
            app-arch/zstd foo
            app-arch/rpm foo
            dev-libs/libffi -foobar
            "
            .into(),
            Precedence::User,
        )?;
        child.inherit_from(&parent.inherit(&grand_parent)?)?;

        assert_eq!(child.0.len(), 5);
        assert_eq!(
            child.0.get(&Atom::new("dev-libs/libffi")?).unwrap(),
            &EntryUseFlags(vec![
                Entry::from_str("foo", Precedence::Profile(0))?,
                Entry::from_str("-bar", Precedence::Profile(0))?,
                Entry::from_str("baz", Precedence::Profile(0))?,
                Entry::from_str("-foobar", Precedence::User)?,
            ])
        );
        assert_eq!(
            child.0.get(&Atom::new("app-arch/zstd")?).unwrap(),
            &EntryUseFlags(vec![
                Entry::from_str("baz", Precedence::Profile(0))?,
                Entry::from_str("foo", Precedence::User)?,
            ])
        );
        assert_eq!(
            child.0.get(&Atom::new("app-arch/rpm")?).unwrap(),
            &EntryUseFlags(vec![Entry::from_str("foo", Precedence::User)?])
        );
        assert_eq!(
            child.0.get(&Atom::new("app-arch/xz-utils")?).unwrap(),
            &EntryUseFlags(vec![
                Entry::from_str("test", Precedence::Profile(1))?,
                Entry::from_str("-foo", Precedence::User)?,
                Entry::from_str("-bar", Precedence::User)?,
                Entry::from_str("baz", Precedence::User)?,
            ])
        );
        Ok(())
    }

    #[test]
    fn test_prefixed_use_flags_inherit_from() -> Result<()> {
        let parent = EntryUseFlags(vec![
            Entry::from_str("foo", Precedence::Profile(0))?,
            Entry::from_str("-qux", Precedence::Profile(0))?,
            Entry::from_str("baz", Precedence::Profile(0))?,
        ]);

        let mut child = EntryUseFlags(vec![
            Entry::from_str("-foo", Precedence::Profile(1))?,
            Entry::from_str("qux", Precedence::Profile(1))?,
        ]);

        child.inherit_from(&parent)?;

        let expected = EntryUseFlags(vec![
            Entry::from_str("baz", Precedence::Profile(0))?,
            Entry::from_str("-foo", Precedence::Profile(1))?,
            Entry::from_str("qux", Precedence::Profile(1))?,
        ]);
        assert_eq!(child, expected);
        Ok(())
    }
}
