use crate::deps::atom::Atom;
use crate::files::content_from_path;
use crate::files::entry::{Entry, Precedence};
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::UseFlag;
use crate::utils::{Inherit, strip_line_comment};
use anyhow::{Context, bail};
use std::path::Path;

#[derive(Clone, Default)]
#[cfg_attr(test, derive(Debug))]
pub struct PackageUseEntries(FxHashMap<Atom, EntryUseFlags>);

impl PackageUseEntries {
    pub fn from_path(path: &Path, order: Precedence, recursive: bool) -> anyhow::Result<Self> {
        let content = content_from_path(path, recursive, true)?;
        Self::from_string(content, order)
            .with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn from_string(content: String, order: Precedence) -> anyhow::Result<Self> {
        let mut map = FxHashMap::default();

        for (lineno, entry) in content.lines().enumerate() {
            let entry = strip_line_comment(entry);
            if entry.is_empty() {
                continue;
            }
            let (atom, flags) = Self::parse_line(entry, order)
                .with_context(|| format!("error in line {}: {entry}", lineno + 1))?;

            let entry: &mut EntryUseFlags = map.entry(atom).or_default();
            entry.update_from(flags);
        }
        Ok(Self(map))
    }

    pub fn into_inner(self) -> FxHashMap<Atom, EntryUseFlags> {
        self.0
    }

    fn parse_line(line: &str, order: Precedence) -> anyhow::Result<(Atom, EntryUseFlags)> {
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

/// Represents a `USE_EXPAND` group, the inner value holds the prefix name.
///
/// At this point we don't distinguish between `USE_EXPAND` and `USE_EXPAND_UNPREFIXED`.
#[derive(Clone, Debug, Eq, PartialEq)]
struct UseExpandGroup(Box<str>);

impl UseExpandGroup {
    fn new(name: &str) -> Self {
        Self(format!("{}_", name.to_ascii_lowercase()).into())
    }

    fn prefix(&self) -> &str {
        &self.0
    }

    /// Expands the given `value` using the prefix and returns a [`UseFlag`].
    fn expand(&self, value: &str) -> anyhow::Result<UseFlag> {
        UseFlag::new(format!("{}{value}", self.0))
    }
}

/// Represents a reset operation for a USE flag or package USE expansion group.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum UseFlagReset {
    All,
    Prefix(Box<str>),
}

impl UseFlagReset {
    fn matches(&self, flag: &UseFlag) -> bool {
        match self {
            Self::All => true,
            Self::Prefix(prefix) => flag.as_str().starts_with(prefix.as_ref()),
        }
    }
}

/// Helper struct to manage USE flags for a single package.
#[derive(Eq, PartialEq, Clone, Default)]
#[cfg_attr(test, derive(Debug))]
pub struct EntryUseFlags {
    flags: FxHashMap<UseFlag, Entry<UseFlag>>,
    resets: FxHashSet<UseFlagReset>,
}

impl EntryUseFlags {
    /// Parses a string of separated USE flags into an [`EntryUseFlags`].
    pub fn from_str(value: &str, order: Precedence) -> anyhow::Result<Self> {
        let mut flags = EntryUseFlags::default();

        let mut cur_expand: Option<UseExpandGroup> = None;
        for flag in value.split_whitespace() {
            if let Some(name) = flag.strip_suffix(':')
                && is_expand_name(name)
            {
                cur_expand = Some(UseExpandGroup::new(name));
                continue;
            }

            if flag == "-*" {
                let reset = cur_expand.as_ref().map_or(UseFlagReset::All, |group| {
                    UseFlagReset::Prefix(group.prefix().into())
                });
                flags.reset(reset);
                continue;
            }

            // Expand the flag if we're in an expansion group
            let entry = match &cur_expand {
                Some(expand) => Self::expanded_entry(flag, expand, order)?,
                None => Entry::from_str(flag, order)?,
            };
            flags.update(entry);
        }
        Ok(flags)
    }

    /// Returns the [`Entry`] for the given `flag`, if it exists.
    pub fn get_match(&self, flag: &UseFlag) -> Option<&Entry<UseFlag>> {
        self.flags.get(flag)
    }

    /// Updates `self` with the given `flag`, replacing an existing flag with the same name.
    fn update(&mut self, flag: Entry<UseFlag>) {
        let name = flag.inner().clone();
        self.flags.insert(name, flag);
    }

    /// Updates `self` with the given [`EntryUseFlags`],
    /// replacing existing flags with the same name.
    fn update_from(&mut self, other: Self) {
        for reset in other.resets {
            self.reset(reset);
        }
        self.flags.extend(other.flags);
    }

    /// Applies the given `reset` to the current flags.
    fn reset(&mut self, reset: UseFlagReset) {
        match &reset {
            UseFlagReset::All => self.flags.clear(),
            UseFlagReset::Prefix(_) => self.flags.retain(|flag, _| !reset.matches(flag)),
        }
        self.resets.insert(reset);
    }

    /// Expands a USE flag entry using the given [`UseExpandGroup`].
    fn expanded_entry(
        value: &str,
        group: &UseExpandGroup,
        order: Precedence,
    ) -> anyhow::Result<Entry<UseFlag>> {
        let entry = match value.strip_prefix('-') {
            Some(value) => format!("-{}", group.expand(value)?),
            None => group.expand(value)?.to_string(),
        };
        Entry::from_str(&entry, order)
    }
}

impl Inherit for EntryUseFlags {
    /// Inherits flags from the given `parent`, replacing any existing flags with the same name.
    /// Flags that are unset in the child will not be inherited from the parent.
    fn inherit_from(&mut self, parent: &Self) -> anyhow::Result<()> {
        for (flag, entry) in &parent.flags {
            if self.resets.iter().any(|reset| reset.matches(flag)) {
                continue;
            }

            self.flags
                .entry(flag.clone())
                .or_insert_with(|| entry.clone());
        }
        self.resets.clear();
        Ok(())
    }
}

/// Checks if the given `name` is a valid expansion group name
fn is_expand_name(name: &str) -> bool {
    match name.as_bytes().split_first() {
        Some((first, rest)) if first.is_ascii_alphabetic() => {
            rest.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_string() -> anyhow::Result<()> {
        let content = "
            # Enable sysvinit symlinks by default.
            sys-apps/systemd sysv-utils
            media-libs/mesa -opencl # ROCm should be the opencl provider

            app-admin/sudo foo -bar baz
            app-admin/sudo -foo
        ";

        let file = PackageUseEntries::from_string(content.into(), Precedence::User)?;
        assert_eq!(file.0.len(), 3);

        let systemd = file.0.get(&Atom::new("sys-apps/systemd")?).unwrap();
        assert_eq!(
            systemd.get_match(&UseFlag::new("sysv-utils")?),
            Some(&Entry::from_str("sysv-utils", Precedence::User)?)
        );

        let mesa = file.0.get(&Atom::new("media-libs/mesa")?).unwrap();
        assert_eq!(
            mesa.get_match(&UseFlag::new("opencl")?),
            Some(&Entry::from_str("-opencl", Precedence::User)?)
        );

        let sudo = file.0.get(&Atom::new("app-admin/sudo")?).unwrap();
        assert_eq!(
            sudo.get_match(&UseFlag::new("foo")?),
            Some(&Entry::from_str("-foo", Precedence::User)?)
        );
        assert_eq!(
            sudo.get_match(&UseFlag::new("bar")?),
            Some(&Entry::from_str("-bar", Precedence::User)?)
        );
        assert_eq!(
            sudo.get_match(&UseFlag::new("baz")?),
            Some(&Entry::from_str("baz", Precedence::User)?)
        );
        Ok(())
    }

    #[test]
    fn test_from_str_expands_use_expand_groups() -> anyhow::Result<()> {
        let flags = EntryUseFlags::from_str(
            "LLVM_TARGETS: X86 -* AMDGPU -WebAssembly PYTHON_TARGETS: python3_14",
            Precedence::User,
        )?;

        assert_eq!(flags.get_match(&UseFlag::new("llvm_targets_X86")?), None);
        assert_eq!(
            flags.get_match(&UseFlag::new("llvm_targets_AMDGPU")?),
            Some(&Entry::from_str("llvm_targets_AMDGPU", Precedence::User)?)
        );
        assert_eq!(
            flags.get_match(&UseFlag::new("llvm_targets_WebAssembly")?),
            Some(&Entry::from_str(
                "-llvm_targets_WebAssembly",
                Precedence::User
            )?)
        );
        assert_eq!(
            flags.get_match(&UseFlag::new("python_targets_python3_14")?),
            Some(&Entry::from_str(
                "python_targets_python3_14",
                Precedence::User
            )?)
        );
        Ok(())
    }

    #[test]
    fn test_from_str_resets_all_flags() -> anyhow::Result<()> {
        let flags = EntryUseFlags::from_str("foo bar -* baz", Precedence::User)?;

        assert_eq!(flags.get_match(&UseFlag::new("foo")?), None);
        assert_eq!(flags.get_match(&UseFlag::new("bar")?), None);
        assert_eq!(
            flags.get_match(&UseFlag::new("baz")?),
            Some(&Entry::from_str("baz", Precedence::User)?)
        );
        Ok(())
    }

    #[test]
    fn test_inherit_from_resets_expand_group() -> anyhow::Result<()> {
        let parent = PackageUseEntries::from_string(
            "dev-lang/rust LLVM_TARGETS: X86".into(),
            Precedence::Profile(0),
        )?;
        let mut child = PackageUseEntries::from_string(
            "dev-lang/rust LLVM_TARGETS: -* AMDGPU".into(),
            Precedence::Profile(1),
        )?;

        child.inherit_from(&parent)?;

        let flags = child.0.values().next().unwrap();
        assert_eq!(flags.get_match(&UseFlag::new("llvm_targets_X86")?), None);
        assert_eq!(
            flags.get_match(&UseFlag::new("llvm_targets_AMDGPU")?),
            Some(&Entry::from_str(
                "llvm_targets_AMDGPU",
                Precedence::Profile(1)
            )?)
        );
        Ok(())
    }

    #[test]
    fn test_resets_are_local_to_atom() -> anyhow::Result<()> {
        let entries = PackageUseEntries::from_string(
            "
            dev-lang/rust LLVM_TARGETS: -* AMDGPU
            */* LLVM_TARGETS: X86
            "
            .into(),
            Precedence::User,
        )?;

        let rust = entries.0.get(&Atom::new("dev-lang/rust")?).unwrap();
        assert_eq!(
            rust.get_match(&UseFlag::new("llvm_targets_AMDGPU")?),
            Some(&Entry::from_str("llvm_targets_AMDGPU", Precedence::User)?)
        );
        assert_eq!(rust.get_match(&UseFlag::new("llvm_targets_X86")?), None);

        let wildcard = entries.0.get(&Atom::new("*/*")?).unwrap();
        assert_eq!(
            wildcard.get_match(&UseFlag::new("llvm_targets_X86")?),
            Some(&Entry::from_str("llvm_targets_X86", Precedence::User)?)
        );
        Ok(())
    }

    #[test]
    fn test_inherit_from_merges_precedence() -> anyhow::Result<()> {
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

        let libffi = child.0.get(&Atom::new("dev-libs/libffi")?).unwrap();
        assert_eq!(
            libffi.get_match(&UseFlag::new("foo")?),
            Some(&Entry::from_str("foo", Precedence::Profile(0))?)
        );
        assert_eq!(
            libffi.get_match(&UseFlag::new("bar")?),
            Some(&Entry::from_str("-bar", Precedence::Profile(0))?)
        );
        assert_eq!(
            libffi.get_match(&UseFlag::new("baz")?),
            Some(&Entry::from_str("baz", Precedence::Profile(0))?)
        );
        assert_eq!(
            libffi.get_match(&UseFlag::new("foobar")?),
            Some(&Entry::from_str("-foobar", Precedence::User)?)
        );

        let zstd = child.0.get(&Atom::new("app-arch/zstd")?).unwrap();
        assert_eq!(
            zstd.get_match(&UseFlag::new("baz")?),
            Some(&Entry::from_str("baz", Precedence::Profile(0))?)
        );
        assert_eq!(
            zstd.get_match(&UseFlag::new("foo")?),
            Some(&Entry::from_str("foo", Precedence::User)?)
        );

        let rpm = child.0.get(&Atom::new("app-arch/rpm")?).unwrap();
        assert_eq!(
            rpm.get_match(&UseFlag::new("foo")?),
            Some(&Entry::from_str("foo", Precedence::User)?)
        );

        let xz = child.0.get(&Atom::new("app-arch/xz-utils")?).unwrap();
        assert_eq!(
            xz.get_match(&UseFlag::new("test")?),
            Some(&Entry::from_str("test", Precedence::Profile(1))?)
        );
        assert_eq!(
            xz.get_match(&UseFlag::new("foo")?),
            Some(&Entry::from_str("-foo", Precedence::User)?)
        );
        assert_eq!(
            xz.get_match(&UseFlag::new("bar")?),
            Some(&Entry::from_str("-bar", Precedence::User)?)
        );
        assert_eq!(
            xz.get_match(&UseFlag::new("baz")?),
            Some(&Entry::from_str("baz", Precedence::User)?)
        );
        Ok(())
    }

    #[test]
    fn test_entry_use_flags_inherit_from() -> anyhow::Result<()> {
        let parent = EntryUseFlags::from_str("foo -qux baz", Precedence::Profile(0))?;
        let mut child = EntryUseFlags::from_str("-foo qux", Precedence::Profile(1))?;

        child.inherit_from(&parent)?;

        assert_eq!(
            child.get_match(&UseFlag::new("baz")?),
            Some(&Entry::from_str("baz", Precedence::Profile(0))?)
        );
        assert_eq!(
            child.get_match(&UseFlag::new("foo")?),
            Some(&Entry::from_str("-foo", Precedence::Profile(1))?)
        );
        assert_eq!(
            child.get_match(&UseFlag::new("qux")?),
            Some(&Entry::from_str("qux", Precedence::Profile(1))?)
        );
        Ok(())
    }
}
