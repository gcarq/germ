use std::path::Path;

use anyhow::{Context, bail};
use germ_pms::{Atom, UseFlag};

use super::{AtomPolicies, AtomPolicy};
use crate::files::entry::{Entry, Precedence};
use crate::makenv::EnvVarName;
use crate::types::FxHashMap;
use crate::useflag::UseExpandConfig;
use crate::utils::Inherit;

/// Parsed `package.use` records.
///
/// These records must be inherited and resolved before they can be used for
/// runtime package USE evaluation.
#[derive(Clone, Default, Debug)]
pub struct PackageUseRecords(AtomPolicies<UseSpec>);

impl PackageUseRecords {
    pub fn from_path(path: &Path, order: Precedence, recursive: bool) -> anyhow::Result<Self> {
        Ok(Self(AtomPolicies::from_path(path, order, recursive)?))
    }

    pub fn from_content(content: &str, order: Precedence) -> anyhow::Result<Self> {
        Ok(Self(AtomPolicies::from_content(content, order)?))
    }

    /// Consumes self, expands all groups and returns all USE flags.
    pub fn expand(self, groups: &UseExpandConfig) -> anyhow::Result<Vec<(Atom, UseFlags)>> {
        self.0
            .into_iter()
            .map(|(atom, spec)| {
                spec.expand(groups)
                    .with_context(|| format!("failed to process USE_EXPAND for {atom}"))
                    .map(|flags| (atom, flags))
            })
            .collect()
    }

    #[cfg(test)]
    fn get(&self, atom: &Atom) -> Option<&UseSpec> {
        self.0.0.get(atom)
    }
}

impl Inherit for PackageUseRecords {
    fn inherit_from(&mut self, parent: &Self) -> anyhow::Result<()> {
        self.0.inherit_from(&parent.0)
    }
}

/// Holds USE flags for a single atom while parsing `package.use` files.
///
/// It contains a mapping of package USE targets to their
/// corresponding [`Entry<UseFlag>`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct UseSpec {
    targets: FxHashMap<PackageUseTarget, Entry<UseFlag>>,
    resets: UseResets,
}

impl UseSpec {
    /// Expands all USE flags, and validates them against the given `groups`.
    fn expand(self, groups: &UseExpandConfig) -> anyhow::Result<UseFlags> {
        let Self { targets, resets } = self;
        let mut flags = FxHashMap::default();
        for (target, entry) in targets {
            let entry = match target {
                PackageUseTarget::Flag(_) => entry,
                PackageUseTarget::Expand { group, .. } => {
                    let flag = groups.resolve_flag(&group, entry.inner())?;
                    entry.replace_inner(flag)
                }
            };
            let flag = entry.inner().clone();
            if flags.contains_key(&flag) {
                bail!("distinct package USE targets resolve to USE flag '{flag}'");
            }
            flags.insert(flag, entry);
        }
        Ok(UseFlags { flags, resets })
    }

    fn reset(&mut self, reset: UseReset, precedence: Precedence) {
        self.targets
            .retain(|target, _| !reset.matches_target(target));
        self.resets.insert(reset, precedence);
    }
}

impl AtomPolicy for UseSpec {
    /// Parses one package USE policy value while retaining expansion groups symbolically.
    fn parse(value: &str, precedence: Precedence) -> anyhow::Result<Self> {
        if value.is_empty() {
            bail!("invalid package.use definition");
        }
        let mut spec = Self::default();
        let mut cur_group: Option<EnvVarName> = None;

        for flag in value.split_whitespace() {
            if let Some(group) = flag.strip_suffix(':')
                && let Ok(group) = EnvVarName::new(group)
            {
                cur_group = Some(group);
                continue;
            }

            if flag == "-*" {
                let reset = cur_group
                    .as_ref()
                    .map_or(UseReset::All, |group| UseReset::Group(group.clone()));
                spec.reset(reset, precedence);
                continue;
            }

            let entry: Entry<UseFlag> = Entry::from_str(flag, precedence)?;
            // Check if we're in an expansion group
            let target = match &cur_group {
                Some(group) => PackageUseTarget::Expand {
                    group: group.clone(),
                    value: entry.inner().clone(),
                },
                None => PackageUseTarget::Flag(entry.inner().clone()),
            };
            spec.targets.insert(target, entry);
        }
        Ok(spec)
    }

    /// Updates `self` with the given [`UseSpec`],
    /// replacing existing flags with the same name.
    fn update_from(&mut self, other: Self) {
        for (reset, precedence) in other.resets.0 {
            self.reset(reset, precedence);
        }
        self.targets.extend(other.targets);
    }
}

impl Inherit for UseSpec {
    /// Inherits parent targets while applying this specification's one-layer resets.
    fn inherit_from(&mut self, parent: &Self) -> anyhow::Result<()> {
        for (target, entry) in &parent.targets {
            if self.resets.matches(target) {
                continue;
            }
            self.targets
                .entry(target.clone())
                .or_insert_with(|| entry.clone());
        }
        self.resets.merge(&parent.resets);
        Ok(())
    }
}

/// Represents a package USE target, which can be either a direct USE flag
/// or within an expansion group.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum PackageUseTarget {
    Flag(UseFlag),
    Expand { group: EnvVarName, value: UseFlag },
}

/// Holds the resolved USE flags for a single atom after expansion and inheritance.
///
/// It maps a [`UseFlag`] to its corresponding [`Entry<UseFlag>`],
/// which contains the operation (set/unset) and precedence.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct UseFlags {
    flags: FxHashMap<UseFlag, Entry<UseFlag>>,
    resets: UseResets,
}

impl UseFlags {
    /// Iterates over the resolved USE flag assignments.
    fn iter(&self) -> impl Iterator<Item = &Entry<UseFlag>> {
        self.flags.values()
    }

    /// Retrieves the [`Entry<UseFlag>`] for the given [`UseFlag`], if it exists.
    #[cfg(test)]
    pub fn get(&self, flag: &UseFlag) -> Option<&Entry<UseFlag>> {
        self.flags.get(flag)
    }
}

/// Merged USE records matched for a single package.
///
/// Entries and resets are combined across matching atoms while keeping the
/// highest [`Precedence`] per entry and reset scope.
#[derive(Debug, Default)]
pub struct MatchedUse<'a> {
    entries: FxHashMap<&'a UseFlag, &'a Entry<UseFlag>>,
    resets: UseResets,
}

impl<'a> MatchedUse<'a> {
    /// Merges `flags`, keeping the highest precedence per entry and reset scope.
    pub fn absorb(&mut self, flags: &'a UseFlags) {
        for entry in flags.iter() {
            match self.entries.get(entry.inner()) {
                Some(existing) if existing.prec > entry.prec => continue,
                _ => self.entries.insert(entry.inner(), entry),
            };
        }
        self.resets.merge(&flags.resets);
    }

    /// Returns the state declared by the matched records for `flag`.
    pub fn state(&self, flag: &UseFlag, groups: &UseExpandConfig) -> Option<bool> {
        let reset = self.resets.precedence(flag, groups);
        match (self.entries.get(flag), reset) {
            (Some(entry), Some(prec)) if entry.prec < prec => Some(false),
            (Some(entry), _) => Some(entry.op.as_bool()),
            (None, Some(_)) => Some(false),
            (None, None) => None,
        }
    }

    /// Consumes self and returns the flags enabled by the matched entries.
    pub fn into_enabled(self) -> impl Iterator<Item = &'a UseFlag> {
        self.entries
            .into_values()
            .filter(|entry| entry.op.as_bool())
            .map(Entry::inner)
    }
}

/// Compiled `-*` reset directives for a set of package USE records.
///
/// Each reset scope keeps the highest [`Precedence`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct UseResets(FxHashMap<UseReset, Precedence>);

impl UseResets {
    /// Merges `other` into `self`, keeping the highest precedence per scope.
    fn merge(&mut self, other: &Self) {
        for (reset, precedence) in &other.0 {
            self.insert(reset.clone(), *precedence);
        }
    }

    /// Returns the highest precedence that matches the given `flag`.
    fn precedence(&self, flag: &UseFlag, groups: &UseExpandConfig) -> Option<Precedence> {
        if self.0.is_empty() {
            return None;
        }
        self.0
            .iter()
            .filter(|(reset, _)| reset.matches_flag(flag, groups))
            .map(|(_, prec)| *prec)
            .max()
    }

    /// Inserts `reset`, keeping the highest precedence for the same scope.
    fn insert(&mut self, reset: UseReset, precedence: Precedence) {
        self.0
            .entry(reset)
            .and_modify(|cur| *cur = (*cur).max(precedence))
            .or_insert(precedence);
    }

    /// Returns whether any reset scope matches `target`.
    fn matches(&self, target: &PackageUseTarget) -> bool {
        self.0.keys().any(|reset| reset.matches_target(target))
    }
}

/// Represents a reset operation for USE flags, which can either reset all flags
/// or flags within an expansion group.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum UseReset {
    All,
    Group(EnvVarName),
}

impl UseReset {
    fn matches_target(&self, target: &PackageUseTarget) -> bool {
        match self {
            Self::All => true,
            Self::Group(name) => match target {
                PackageUseTarget::Expand { group, .. } => name == group,
                _ => false,
            },
        }
    }

    /// Returns whether this reset matches the given USE flag, considering expansion groups.
    fn matches_flag(&self, flag: &UseFlag, groups: &UseExpandConfig) -> bool {
        match self {
            Self::All => true,
            Self::Group(name) => groups.group_matches_flag(name, flag),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::makenv::MakeEnv;
    use crate::types::FxHashSet;

    impl PackageUseTarget {
        fn flag(value: impl Into<Box<str>>) -> anyhow::Result<Self> {
            Ok(Self::Flag(UseFlag::new(value)?))
        }

        fn expand(group: &str, value: impl Into<Box<str>>) -> anyhow::Result<Self> {
            Ok(Self::Expand {
                group: EnvVarName::new(group)?,
                value: UseFlag::new(value)?,
            })
        }
    }

    fn config() -> anyhow::Result<UseExpandConfig> {
        let makenv = MakeEnv::from_content(
            "USE_EXPAND=\"LLVM_TARGETS\"
            USE_EXPAND_UNPREFIXED=\"ARCH\"",
        )?;
        UseExpandConfig::from_makenv(&makenv)
    }

    fn flags_for<'a>(rules: &'a [(Atom, UseFlags)], atom: &str) -> anyhow::Result<&'a UseFlags> {
        let atom = Atom::new(atom)?;
        rules
            .iter()
            .find_map(|(rule_atom, flags)| (rule_atom == &atom).then_some(flags))
            .context("missing package USE rule")
    }

    #[test]
    fn test_parse_duplicate_atom_updates_flags() -> anyhow::Result<()> {
        let policy = PackageUseRecords::from_content(
            "app-admin/sudo foo -bar baz
                app-admin/sudo -foo",
            Precedence::User,
        )?;
        let sudo = policy.get(&Atom::new("app-admin/sudo")?).unwrap();

        assert_eq!(
            sudo.targets.get(&PackageUseTarget::flag("foo")?),
            Some(&Entry::from_str("-foo", Precedence::User)?)
        );
        assert_eq!(
            sudo.targets.get(&PackageUseTarget::flag("bar")?),
            Some(&Entry::from_str("-bar", Precedence::User)?)
        );
        assert_eq!(
            sudo.targets.get(&PackageUseTarget::flag("baz")?),
            Some(&Entry::from_str("baz", Precedence::User)?)
        );
        Ok(())
    }

    #[test]
    fn test_parse_keeps_expansion_symbolic() -> anyhow::Result<()> {
        let spec = UseSpec::parse(
            "foo llvm_targets_AMDGPU LLVM_TARGETS: AMDGPU ARCH: amd64 LLVM_TARGETS:",
            Precedence::User,
        )?;

        assert!(spec.targets.contains_key(&PackageUseTarget::flag("foo")?));
        assert!(
            spec.targets
                .contains_key(&PackageUseTarget::flag("llvm_targets_AMDGPU")?)
        );
        assert!(
            spec.targets
                .contains_key(&PackageUseTarget::expand("LLVM_TARGETS", "AMDGPU")?)
        );
        assert!(
            spec.targets
                .contains_key(&PackageUseTarget::expand("ARCH", "amd64")?)
        );
        assert!(spec.resets.0.is_empty());
        Ok(())
    }

    #[test]
    fn test_parse_group_context_is_line_local() -> anyhow::Result<()> {
        let entries = PackageUseRecords::from_content(
            "dev-lang/rust LLVM_TARGETS: AMDGPU
                app-arch/xz-utils direct_flag",
            Precedence::User,
        )?;

        let xz = entries.get(&Atom::new("app-arch/xz-utils")?).unwrap();
        assert!(
            xz.targets
                .contains_key(&PackageUseTarget::flag("direct_flag")?)
        );
        Ok(())
    }

    #[test]
    fn test_parse_resets() -> anyhow::Result<()> {
        let spec = UseSpec::parse("foo bar -* baz", Precedence::User)?;

        assert!(!spec.targets.contains_key(&PackageUseTarget::flag("foo")?));
        assert!(!spec.targets.contains_key(&PackageUseTarget::flag("bar")?));
        assert!(spec.targets.contains_key(&PackageUseTarget::flag("baz")?));
        assert!(spec.resets.0.contains_key(&UseReset::All));

        let spec = UseSpec::parse("LLVM_TARGETS: X86 -* AMDGPU", Precedence::User)?;
        assert!(
            !spec
                .targets
                .contains_key(&PackageUseTarget::expand("LLVM_TARGETS", "X86")?)
        );
        assert!(
            spec.targets
                .contains_key(&PackageUseTarget::expand("LLVM_TARGETS", "AMDGPU")?)
        );
        assert!(
            spec.resets
                .0
                .contains_key(&UseReset::Group(EnvVarName::new("LLVM_TARGETS")?))
        );
        Ok(())
    }

    #[test]
    fn test_resolve_expansion_groups() -> anyhow::Result<()> {
        let entries = PackageUseRecords::from_content(
            "dev-lang/rust LLVM_TARGETS: WebAssembly -AMDGPU ARCH: amd64 -x86",
            Precedence::Profile(2),
        )?;
        let resolved = entries.expand(&config()?)?;
        let flags = flags_for(&resolved, "dev-lang/rust")?;

        assert_eq!(
            flags.get(&UseFlag::new("llvm_targets_WebAssembly")?),
            Some(&Entry::from_str(
                "llvm_targets_WebAssembly",
                Precedence::Profile(2)
            )?)
        );
        assert_eq!(
            flags.get(&UseFlag::new("llvm_targets_AMDGPU")?),
            Some(&Entry::from_str(
                "-llvm_targets_AMDGPU",
                Precedence::Profile(2),
            )?)
        );
        assert_eq!(
            flags.get(&UseFlag::new("amd64")?),
            Some(&Entry::from_str("amd64", Precedence::Profile(2))?)
        );
        assert_eq!(
            flags.get(&UseFlag::new("x86")?),
            Some(&Entry::from_str("-x86", Precedence::Profile(2))?)
        );
        Ok(())
    }

    #[test]
    fn test_resolve_rejects_invalid_groups() -> anyhow::Result<()> {
        let cases = [
            "dev-lang/rust UNKNOWN: value",
            "dev-lang/rust llvm_targets: AMDGPU",
            "dev-lang/rust foo ARCH: foo",
        ];
        for line in cases {
            let entries = PackageUseRecords::from_content(line, Precedence::User)?;
            assert!(entries.expand(&config()?).is_err(), "{line}");
        }

        let trailing = PackageUseRecords::from_content("dev-lang/rust UNKNOWN:", Precedence::User)?;
        assert!(trailing.expand(&config()?).is_ok());
        Ok(())
    }

    #[test]
    fn test_inherit_ignores_reset_only_group() -> anyhow::Result<()> {
        let parent = PackageUseRecords::from_content(
            "dev-lang/rust LLVM_TARGETS: X86",
            Precedence::Profile(0),
        )?;
        let child = PackageUseRecords::from_content("dev-lang/rust UNKNOWN: -*", Precedence::User)?
            .inherit(&parent)?;
        let resolved = child.expand(&config()?)?;
        let flags = flags_for(&resolved, "dev-lang/rust")?;

        assert_eq!(
            flags.get(&UseFlag::new("llvm_targets_X86")?),
            Some(&Entry::from_str(
                "llvm_targets_X86",
                Precedence::Profile(0)
            )?)
        );
        Ok(())
    }

    #[test]
    fn test_inherit_group_reset() -> anyhow::Result<()> {
        let grand_parent = PackageUseRecords::from_content(
            "dev-lang/rust lto LLVM_TARGETS: X86",
            Precedence::Profile(0),
        )?;
        let parent = PackageUseRecords::from_content(
            "dev-lang/rust -lto LLVM_TARGETS: -* AMDGPU",
            Precedence::Profile(1),
        )?
        .inherit(&grand_parent)?;

        let parent_flags = parent.clone().expand(&config()?)?;
        let parent_flags = flags_for(&parent_flags, "dev-lang/rust")?;
        assert_eq!(
            parent_flags.get(&UseFlag::new("lto")?),
            Some(&Entry::from_str("-lto", Precedence::Profile(1))?)
        );
        assert_eq!(parent_flags.get(&UseFlag::new("llvm_targets_X86")?), None);
        assert_eq!(
            parent_flags.get(&UseFlag::new("llvm_targets_AMDGPU")?),
            Some(&Entry::from_str(
                "llvm_targets_AMDGPU",
                Precedence::Profile(1)
            )?)
        );

        let child = PackageUseRecords::from_content(
            "dev-lang/rust lto LLVM_TARGETS: -* WebAssembly",
            Precedence::User,
        )?
        .inherit(&parent)?;
        let flags = child.expand(&config()?)?;
        let flags = flags_for(&flags, "dev-lang/rust")?;
        assert_eq!(
            flags.get(&UseFlag::new("lto")?),
            Some(&Entry::from_str("lto", Precedence::User)?)
        );
        assert_eq!(flags.get(&UseFlag::new("llvm_targets_X86")?), None);
        assert_eq!(flags.get(&UseFlag::new("llvm_targets_AMDGPU")?), None);
        assert_eq!(
            flags.get(&UseFlag::new("llvm_targets_WebAssembly")?),
            Some(&Entry::from_str(
                "llvm_targets_WebAssembly",
                Precedence::User
            )?)
        );
        Ok(())
    }

    #[test]
    fn test_resets_are_local_to_atom() -> anyhow::Result<()> {
        let entries = PackageUseRecords::from_content(
            "
            dev-lang/rust LLVM_TARGETS: -* AMDGPU
            */* LLVM_TARGETS: X86
            ",
            Precedence::User,
        )?;
        let resolved = entries.expand(&config()?)?;

        let rust = flags_for(&resolved, "dev-lang/rust")?;
        assert_eq!(
            rust.get(&UseFlag::new("llvm_targets_AMDGPU")?),
            Some(&Entry::from_str("llvm_targets_AMDGPU", Precedence::User)?)
        );
        assert_eq!(rust.get(&UseFlag::new("llvm_targets_X86")?), None);

        let wildcard = flags_for(&resolved, "*/*")?;
        assert_eq!(
            wildcard.get(&UseFlag::new("llvm_targets_X86")?),
            Some(&Entry::from_str("llvm_targets_X86", Precedence::User)?)
        );
        Ok(())
    }

    #[test]
    fn test_matched_use() -> anyhow::Result<()> {
        let config = config()?;
        let profile = PackageUseRecords::from_content(
            "llvm-core/clang LLVM_TARGETS: NVPTX",
            Precedence::Profile(0),
        )?
        .expand(&config)?;
        let user =
            PackageUseRecords::from_content("*/* LLVM_TARGETS: -* AMDGPU", Precedence::User)?
                .expand(&config)?;
        let mut matched = MatchedUse::default();
        for (_, flags) in profile.iter().chain(user.iter()) {
            matched.absorb(flags);
        }

        for (flag, expected) in [
            ("llvm_targets_NVPTX", Some(false)),
            ("llvm_targets_AMDGPU", Some(true)),
            ("llvm_targets_X86", Some(false)),
            ("lto", None),
        ] {
            assert_eq!(matched.state(&UseFlag::new(flag)?, &config), expected);
        }

        assert_eq!(
            matched.into_enabled().cloned().collect::<FxHashSet<_>>(),
            FxHashSet::from_iter([
                UseFlag::new("llvm_targets_NVPTX")?,
                UseFlag::new("llvm_targets_AMDGPU")?,
            ])
        );
        Ok(())
    }
}
