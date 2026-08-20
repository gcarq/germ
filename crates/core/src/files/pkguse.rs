use crate::deps::atom::Atom;
use crate::files::content_from_path;
use crate::files::entry::{Entry, Precedence};
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::{UseExpandConfig, UseFlag};
use crate::utils::{Inherit, strip_line_comment};
use anyhow::{Context, bail};
use std::path::Path;

/// Represents the content of a package USE file.
///
/// This should not be used as source of truth for USE flags,
/// but rather as a representation of the package USE file content.
///
/// It can be only used as USE flags lookup after inheriting all files
/// and calling `PackageUseEntries::expand` to expand the USE flags.
#[derive(Clone, Default)]
#[cfg_attr(test, derive(Debug))]
pub struct PackageUseEntries(FxHashMap<Atom, UseSpec>);

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

            let entry: &mut UseSpec = map.entry(atom).or_default();
            entry.update_from(flags);
        }
        Ok(Self(map))
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

    fn parse_line(line: &str, order: Precedence) -> anyhow::Result<(Atom, UseSpec)> {
        match line.split_once(char::is_whitespace) {
            Some((atom, flags)) => Ok((atom.parse()?, UseSpec::from_str(flags, order)?)),
            None => bail!("invalid package.use definition: {line}"),
        }
    }
}

impl Inherit for PackageUseEntries {
    fn inherit_from(&mut self, parent: &Self) -> anyhow::Result<()> {
        for (atom, parent) in &parent.0 {
            if let Some(this) = self.0.get_mut(atom) {
                this.inherit_from(parent)?;
            } else {
                self.0.insert(atom.clone(), parent.clone());
            }
        }
        Ok(())
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
    /// Parses one package USE policy value while retaining expansion groups symbolically.
    fn from_str(value: &str, order: Precedence) -> anyhow::Result<Self> {
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
            let entry: Entry<UseFlag> = Entry::from_str(flag, order)?;
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
    use crate::files::entry::Operation;
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
            systemd.targets.get(&PackageUseTarget::flag("sysv-utils")?),
            Some(&Entry::from_str("sysv-utils", Precedence::User)?)
        );

        let mesa = file.0.get(&Atom::new("media-libs/mesa")?).unwrap();
        assert_eq!(
            mesa.targets.get(&PackageUseTarget::flag("opencl")?),
            Some(&Entry::from_str("-opencl", Precedence::User)?)
        );

        let sudo = file.0.get(&Atom::new("app-admin/sudo")?).unwrap();
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
        let spec = UseSpec::from_str(
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
        let entries = PackageUseEntries::from_string(
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
        let spec = UseSpec::from_str("foo bar -* baz", Precedence::User)?;

        assert!(!spec.targets.contains_key(&PackageUseTarget::flag("foo")?));
        assert!(!spec.targets.contains_key(&PackageUseTarget::flag("bar")?));
        assert!(spec.targets.contains_key(&PackageUseTarget::flag("baz")?));
        assert!(spec.resets.contains(&UseReset::All));

        let spec = UseSpec::from_str("LLVM_TARGETS: X86 -* AMDGPU", Precedence::User)?;
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
        let entries = PackageUseEntries::from_string(
            "dev-lang/rust LLVM_TARGETS: WebAssembly -AMDGPU ARCH: amd64 -x86".into(),
            Precedence::User,
        )?;
        let resolved = entries.expand(&config()?)?;
        let flags = resolved.get(&Atom::new("dev-lang/rust")?).unwrap();

        assert_eq!(
            flags.get(&UseFlag::new("llvm_targets_WebAssembly")?),
            Some(&Entry::from_str(
                "llvm_targets_WebAssembly",
                Precedence::User
            )?)
        );
        assert_eq!(
            flags.get(&UseFlag::new("llvm_targets_AMDGPU")?),
            Some(&Entry::from_str("-llvm_targets_AMDGPU", Precedence::User)?)
        );
        assert_eq!(
            flags.get(&UseFlag::new("amd64")?),
            Some(&Entry::from_str("amd64", Precedence::User)?)
        );
        assert_eq!(
            flags.get(&UseFlag::new("x86")?),
            Some(&Entry::from_str("-x86", Precedence::User)?)
        );
        Ok(())
    }

    #[test]
    fn test_resolve_preserves_operation_and_precedence() -> anyhow::Result<()> {
        let entries = PackageUseEntries::from_string(
            "dev-lang/rust LLVM_TARGETS: AMDGPU".into(),
            Precedence::Profile(2),
        )?;
        let resolved = entries.expand(&config()?)?;
        let entry = resolved
            .get(&Atom::new("dev-lang/rust")?)
            .unwrap()
            .get(&UseFlag::new("llvm_targets_AMDGPU")?)
            .unwrap();

        assert_eq!(entry.op, Operation::Set);
        assert_eq!(entry.prec, Precedence::Profile(2));
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
            let entries = PackageUseEntries::from_string(line.into(), Precedence::User)?;
            assert!(entries.expand(&config()?).is_err(), "{line}");
        }

        let trailing =
            PackageUseEntries::from_string("dev-lang/rust UNKNOWN:".into(), Precedence::User)?;
        assert!(trailing.expand(&config()?).is_ok());

        Ok(())
    }

    #[test]
    fn test_resolve_ignores_reset_only_group() -> anyhow::Result<()> {
        let entries =
            PackageUseEntries::from_string("dev-lang/rust UNKNOWN: -*".into(), Precedence::User)?;
        let resolved = entries.expand(&config()?)?;

        let flags = resolved.get(&Atom::new("dev-lang/rust")?).unwrap();
        assert!(flags.flags.is_empty());
        Ok(())
    }

    #[test]
    fn test_inherit_ignores_reset_only_group() -> anyhow::Result<()> {
        let parent = PackageUseEntries::from_string(
            "dev-lang/rust LLVM_TARGETS: X86".into(),
            Precedence::Profile(0),
        )?;
        let child =
            PackageUseEntries::from_string("dev-lang/rust UNKNOWN: -*".into(), Precedence::User)?
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
    fn test_resolve_rejects_overlapping_namespaces() -> anyhow::Result<()> {
        let make_env =
            MakeEnv::from_string("USE_EXPAND=\"ARCH\"\nUSE_EXPAND_UNPREFIXED=\"ARCH\"".into())?;
        assert!(UseExpandConfig::from_make_env(&make_env).is_err());
        Ok(())
    }

    #[test]
    fn test_inherit_from_resets_and_readds() -> anyhow::Result<()> {
        let grand_parent = PackageUseEntries::from_string(
            "dev-lang/rust LLVM_TARGETS: X86".into(),
            Precedence::Profile(0),
        )?;
        let parent = PackageUseEntries::from_string(
            "dev-lang/rust LLVM_TARGETS: -* AMDGPU".into(),
            Precedence::Profile(1),
        )?;
        let parent = parent.inherit(&grand_parent)?;
        let child = PackageUseEntries::default().inherit(&parent)?;
        let flags = child.expand(&config()?)?;
        let flags = flags.get(&Atom::new("dev-lang/rust")?).unwrap();

        assert_eq!(flags.get(&UseFlag::new("llvm_targets_X86")?), None);
        assert_eq!(
            flags.get(&UseFlag::new("llvm_targets_AMDGPU")?),
            Some(&Entry::from_str(
                "llvm_targets_AMDGPU",
                Precedence::Profile(1)
            )?)
        );
        Ok(())
    }

    #[test]
    fn test_inherit_from_keeps_group_reset_local() -> anyhow::Result<()> {
        let parent = PackageUseEntries::from_string(
            "dev-lang/rust foo LLVM_TARGETS: X86".into(),
            Precedence::Profile(0),
        )?;
        let child = PackageUseEntries::from_string(
            "dev-lang/rust LLVM_TARGETS: -* AMDGPU".into(),
            Precedence::Profile(1),
        )?
        .inherit(&parent)?;
        let flags = child.expand(&config()?)?;
        let flags = flags.get(&Atom::new("dev-lang/rust")?).unwrap();

        assert_eq!(
            flags.get(&UseFlag::new("foo")?),
            Some(&Entry::from_str("foo", Precedence::Profile(0))?)
        );
        assert_eq!(flags.get(&UseFlag::new("llvm_targets_X86")?), None);
        assert_eq!(
            flags.get(&UseFlag::new("llvm_targets_AMDGPU")?),
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

        let child = PackageUseEntries::from_string(
            "
            app-arch/xz-utils -foo -bar baz
            app-arch/zstd foo
            app-arch/rpm foo
            dev-libs/libffi -foobar
            "
            .into(),
            Precedence::User,
        )?;
        let child = child.inherit(&parent.inherit(&grand_parent)?)?;
        let resolved = child.expand(&config()?)?;

        assert_eq!(resolved.len(), 5);

        let libffi = resolved.get(&Atom::new("dev-libs/libffi")?).unwrap();
        assert_eq!(
            libffi.get(&UseFlag::new("foo")?),
            Some(&Entry::from_str("foo", Precedence::Profile(0))?)
        );
        assert_eq!(
            libffi.get(&UseFlag::new("bar")?),
            Some(&Entry::from_str("-bar", Precedence::Profile(0))?)
        );
        assert_eq!(
            libffi.get(&UseFlag::new("baz")?),
            Some(&Entry::from_str("baz", Precedence::Profile(0))?)
        );
        assert_eq!(
            libffi.get(&UseFlag::new("foobar")?),
            Some(&Entry::from_str("-foobar", Precedence::User)?)
        );

        let zstd = resolved.get(&Atom::new("app-arch/zstd")?).unwrap();
        assert_eq!(
            zstd.get(&UseFlag::new("baz")?),
            Some(&Entry::from_str("baz", Precedence::Profile(0))?)
        );
        assert_eq!(
            zstd.get(&UseFlag::new("foo")?),
            Some(&Entry::from_str("foo", Precedence::User)?)
        );

        let rpm = resolved.get(&Atom::new("app-arch/rpm")?).unwrap();
        assert_eq!(
            rpm.get(&UseFlag::new("foo")?),
            Some(&Entry::from_str("foo", Precedence::User)?)
        );
        Ok(())
    }
}
