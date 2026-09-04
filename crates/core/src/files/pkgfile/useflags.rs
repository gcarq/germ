use super::{AtomPolicies, AtomPolicy};
use crate::deps::atom::Atom;
use crate::files::entry::{Entry, Precedence};
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::{UseExpandConfig, UseFlag};
use crate::utils::Inherit;
use anyhow::{Context, bail};
use std::path::Path;

/// Represents the content of a package USE file.
///
/// This should not be used as source of truth for USE flags,
/// but rather as a representation of the package USE file content.
///
/// It can be only used as USE flags lookup after inheriting all files
/// and calling `PackageUsePolicy::expand` to expand the USE flags.
#[derive(Clone, Default, Debug)]
pub struct PackageUsePolicy(AtomPolicies<UseSpec>);

impl PackageUsePolicy {
    pub fn from_path(path: &Path, order: Precedence, recursive: bool) -> anyhow::Result<Self> {
        Ok(Self(AtomPolicies::from_path(path, order, recursive)?))
    }

    pub fn from_string(content: String, order: Precedence) -> anyhow::Result<Self> {
        Ok(Self(AtomPolicies::from_string(content, order)?))
    }

    /// Expands all USE flags, and validates them against the given `groups`.
    pub fn expand(self, groups: &UseExpandConfig) -> anyhow::Result<FxHashMap<Atom, UseFlags>> {
        self.0
            .into_iter()
            .map(|(atom, spec)| {
                spec.expand(groups)
                    .with_context(|| format!("failed to resolve package USE policy for {atom}"))
                    .map(|flags| (atom, flags))
            })
            .collect()
    }
}

impl Inherit for PackageUsePolicy {
    fn inherit_from(&mut self, parent: &Self) -> anyhow::Result<()> {
        self.0.inherit_from(&parent.0)
    }
}

/// Represents a package USE target, which can be either a direct USE flag
/// or within an expansion group.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum PackageUseTarget {
    Flag(UseFlag),
    Expand { group: Box<str>, value: UseFlag },
}

/// Represents a reset operation for USE flags, which can either reset all flags
/// or flags within an expansion group.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum UseReset {
    All,
    Group(Box<str>),
}

impl UseReset {
    fn matches(&self, target: &PackageUseTarget) -> bool {
        match self {
            Self::All => true,
            Self::Group(group) => match target {
                PackageUseTarget::Expand {
                    group: target_group,
                    ..
                } => group.as_ref() == target_group.as_ref(),
                _ => false,
            },
        }
    }
}

/// Helper struct to manage USE flags for a single atom,
/// while parsing `package.use` files.
///
/// It contains a mapping of package USE targets to their
/// corresponding (expanded) [`Entry<UseFlag>`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct UseSpec {
    targets: FxHashMap<PackageUseTarget, Entry<UseFlag>>,
    resets: FxHashSet<UseReset>,
}

impl UseSpec {
    /// Expands all USE flags, and validates them against the given `groups`.
    fn expand(self, groups: &UseExpandConfig) -> anyhow::Result<UseFlags> {
        let mut flags = FxHashMap::default();
        for (target, entry) in self.targets {
            let entry = match target {
                PackageUseTarget::Flag(_) => entry,
                PackageUseTarget::Expand { group, .. } => groups.expand_entry(&group, entry)?,
            };
            let flag = entry.inner().clone();
            if flags.contains_key(&flag) {
                bail!("distinct package USE targets resolve to USE flag '{flag}'");
            }
            flags.insert(flag, entry);
        }
        Ok(UseFlags { flags })
    }

    fn reset(&mut self, reset: UseReset) {
        self.targets.retain(|target, _| !reset.matches(target));
        self.resets.insert(reset);
    }
}

impl AtomPolicy for UseSpec {
    /// Parses one package USE policy value while retaining expansion groups symbolically.
    fn parse(value: &str, precedence: Precedence) -> anyhow::Result<Self> {
        if value.is_empty() {
            bail!("invalid package.use definition");
        }
        let mut spec = Self::default();
        let mut cur_group: Option<Box<str>> = None;

        for flag in value.split_whitespace() {
            if let Some(group) = flag.strip_suffix(':')
                && is_expand_name(group)
            {
                cur_group = Some(group.into());
                continue;
            }

            if flag == "-*" {
                let reset = cur_group
                    .as_ref()
                    .map_or(UseReset::All, |group| UseReset::Group(group.clone()));
                spec.reset(reset);
                continue;
            }

            // Expand the flag if we're in an expansion group
            let entry: Entry<UseFlag> = Entry::from_str(flag, precedence)?;
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
        for reset in other.resets {
            self.reset(reset);
        }
        self.targets.extend(other.targets);
    }
}

impl Inherit for UseSpec {
    /// Inherits parent targets while applying this specification's one-layer resets.
    fn inherit_from(&mut self, parent: &Self) -> anyhow::Result<()> {
        for (target, entry) in &parent.targets {
            if self.resets.iter().any(|reset| reset.matches(target)) {
                continue;
            }
            self.targets
                .entry(target.clone())
                .or_insert_with(|| entry.clone());
        }
        self.resets.clear();
        Ok(())
    }
}

/// Represents the final resolved USE flags for a package after expansion and inheritance.
///
/// It maps a [`UseFlag`] to its corresponding [`Entry<UseFlag>`],
/// which contains the operation (set/unset) and precedence.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct UseFlags {
    flags: FxHashMap<UseFlag, Entry<UseFlag>>,
}

impl UseFlags {
    /// Retrieves the [`Entry<UseFlag>`] for the given [`UseFlag`], if it exists.
    pub fn get(&self, flag: &UseFlag) -> Option<&Entry<UseFlag>> {
        self.flags.get(flag)
    }
}

/// Checks if the given `name` is a valid package USE expansion group name.
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

    use crate::makenv::MakeEnv;

    impl PackageUseTarget {
        fn flag(value: impl Into<Box<str>>) -> anyhow::Result<Self> {
            Ok(Self::Flag(UseFlag::new(value)?))
        }

        fn expand(group: impl Into<Box<str>>, value: impl Into<Box<str>>) -> anyhow::Result<Self> {
            Ok(Self::Expand {
                group: group.into(),
                value: UseFlag::new(value)?,
            })
        }
    }

    fn config() -> anyhow::Result<UseExpandConfig> {
        let make_env = MakeEnv::from_string(
            "USE_EXPAND=\"LLVM_TARGETS\"\nUSE_EXPAND_UNPREFIXED=\"ARCH\"".into(),
        )?;
        UseExpandConfig::from_make_env(&make_env)
    }

    #[test]
    fn test_parse_duplicate_atom() -> anyhow::Result<()> {
        let policy = PackageUsePolicy::from_string(
            "app-admin/sudo foo -bar baz
                app-admin/sudo -foo"
                .into(),
            Precedence::User,
        )?;
        let sudo = policy.0.get(&Atom::new("app-admin/sudo")?).unwrap();

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
        assert!(spec.resets.is_empty());
        Ok(())
    }

    #[test]
    fn test_parse_group_context_is_line_local() -> anyhow::Result<()> {
        let entries = PackageUsePolicy::from_string(
            "dev-lang/rust LLVM_TARGETS: AMDGPU\napp-arch/xz-utils direct_flag".into(),
            Precedence::User,
        )?;

        let xz = entries.0.get(&Atom::new("app-arch/xz-utils")?).unwrap();
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
        assert!(spec.resets.contains(&UseReset::All));

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
                .contains(&UseReset::Group("LLVM_TARGETS".into()))
        );
        Ok(())
    }

    #[test]
    fn test_resolve_expansion_groups() -> anyhow::Result<()> {
        let entries = PackageUsePolicy::from_string(
            "dev-lang/rust LLVM_TARGETS: WebAssembly -AMDGPU ARCH: amd64 -x86".into(),
            Precedence::Profile(2),
        )?;
        let resolved = entries.expand(&config()?)?;
        let flags = resolved.get(&Atom::new("dev-lang/rust")?).unwrap();

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
            let entries = PackageUsePolicy::from_string(line.into(), Precedence::User)?;
            assert!(entries.expand(&config()?).is_err(), "{line}");
        }

        let trailing =
            PackageUsePolicy::from_string("dev-lang/rust UNKNOWN:".into(), Precedence::User)?;
        assert!(trailing.expand(&config()?).is_ok());

        Ok(())
    }

    #[test]
    fn test_inherit_ignores_reset_only_group() -> anyhow::Result<()> {
        let parent = PackageUsePolicy::from_string(
            "dev-lang/rust LLVM_TARGETS: X86".into(),
            Precedence::Profile(0),
        )?;
        let child =
            PackageUsePolicy::from_string("dev-lang/rust UNKNOWN: -*".into(), Precedence::User)?
                .inherit(&parent)?;
        let resolved = child.expand(&config()?)?;
        let flags = resolved.get(&Atom::new("dev-lang/rust")?).unwrap();

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
    fn test_resolve_rejects_overlapping_groups() -> anyhow::Result<()> {
        let make_env = MakeEnv::from_string(
            "USE_EXPAND=\"ARCH\"
                USE_EXPAND_UNPREFIXED=\"ARCH\""
                .into(),
        )?;
        assert!(UseExpandConfig::from_make_env(&make_env).is_err());
        Ok(())
    }

    #[test]
    fn test_inherit_group_reset() -> anyhow::Result<()> {
        let grand_parent = PackageUsePolicy::from_string(
            "dev-lang/rust lto LLVM_TARGETS: X86".into(),
            Precedence::Profile(0),
        )?;
        let parent = PackageUsePolicy::from_string(
            "dev-lang/rust -lto LLVM_TARGETS: -* AMDGPU".into(),
            Precedence::Profile(1),
        )?
        .inherit(&grand_parent)?;

        let parent_flags = parent.clone().expand(&config()?)?;
        let parent_flags = parent_flags.get(&Atom::new("dev-lang/rust")?).unwrap();
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

        let child = PackageUsePolicy::from_string(
            "dev-lang/rust lto LLVM_TARGETS: -* WebAssembly".into(),
            Precedence::User,
        )?
        .inherit(&parent)?;
        let flags = child.expand(&config()?)?;
        let flags = flags.get(&Atom::new("dev-lang/rust")?).unwrap();
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
        let entries = PackageUsePolicy::from_string(
            "
            dev-lang/rust LLVM_TARGETS: -* AMDGPU
            */* LLVM_TARGETS: X86
            "
            .into(),
            Precedence::User,
        )?;
        let resolved = entries.expand(&config()?)?;

        let rust = resolved.get(&Atom::new("dev-lang/rust")?).unwrap();
        assert_eq!(
            rust.get(&UseFlag::new("llvm_targets_AMDGPU")?),
            Some(&Entry::from_str("llvm_targets_AMDGPU", Precedence::User)?)
        );
        assert_eq!(rust.get(&UseFlag::new("llvm_targets_X86")?), None);

        let wildcard = resolved.get(&Atom::new("*/*")?).unwrap();
        assert_eq!(
            wildcard.get(&UseFlag::new("llvm_targets_X86")?),
            Some(&Entry::from_str("llvm_targets_X86", Precedence::User)?)
        );
        Ok(())
    }
}
