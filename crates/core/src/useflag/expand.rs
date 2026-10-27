use anyhow::{Context, bail};
use germ_pms::UseFlag;

use crate::makenv::{EnvValue, EnvVarName, MakeEnv};
use crate::types::{FxHashMap, FxHashSet};

/// A declared USE expansion group and the namespace it expands into.
#[derive(Clone, Debug)]
enum ExpandGroup {
    /// Prefixed group mapped to its lowercase `NAME_` flag prefix.
    Prefixed { prefix: Box<str> },
    /// Unprefixed group mapped to its `USE_EXPAND_VALUES_<NAME>` values.
    Unprefixed { values: FxHashSet<Box<str>> },
}

impl ExpandGroup {
    /// Creates a prefixed group for the given `name`.
    fn prefixed(name: &EnvVarName) -> Self {
        Self::Prefixed {
            prefix: format!("{}_", name.as_str().to_ascii_lowercase()).into_boxed_str(),
        }
    }

    /// Expands one group value into its corresponding USE flag.
    fn flag(&self, value: &str) -> anyhow::Result<UseFlag> {
        match self {
            Self::Prefixed { prefix } => UseFlag::new(format!("{prefix}{value}")),
            Self::Unprefixed { .. } => UseFlag::new(value),
        }
    }
}

/// Holds the declared USE expansion groups and the implicit group names.
///
/// A group is declared in exactly one of `USE_EXPAND` and
/// `USE_EXPAND_UNPREFIXED`.
#[derive(Clone, Debug, Default)]
pub struct UseExpandConfig {
    /// Declared groups mapped to their namespaced definition.
    groups: FxHashMap<EnvVarName, ExpandGroup>,
    /// Implicit groups get injected into `IUSE_IMPLICIT`.
    implicit_groups: FxHashSet<EnvVarName>,
}

impl UseExpandConfig {
    /// Builds the expansion config from the effective [`MakeEnv`].
    pub fn from_makenv(makenv: &MakeEnv) -> anyhow::Result<Self> {
        let prefixed = parse_groups("USE_EXPAND", makenv.get("USE_EXPAND"))?;
        let unprefixed =
            parse_groups("USE_EXPAND_UNPREFIXED", makenv.get("USE_EXPAND_UNPREFIXED"))?;
        if let Some(name) = prefixed.intersection(&unprefixed).next() {
            bail!("USE expansion '{name}' is present in both USE_EXPAND namespaces");
        }

        let mut groups = prefixed
            .into_iter()
            .map(|name| {
                let group = ExpandGroup::prefixed(&name);
                (name, group)
            })
            .collect::<FxHashMap<_, _>>();
        for name in unprefixed {
            let values = makenv
                .get(format!("USE_EXPAND_VALUES_{name}").as_str())
                .into_iter()
                .flat_map(EnvValue::iter)
                .map(Into::into)
                .collect();
            groups.insert(name, ExpandGroup::Unprefixed { values });
        }

        let implicit_groups =
            parse_groups("USE_EXPAND_IMPLICIT", makenv.get("USE_EXPAND_IMPLICIT"))?;

        Ok(Self {
            groups,
            implicit_groups,
        })
    }

    /// Returns the USE expansion group and value for `flag`,
    /// preferring the longest matching group name.
    pub fn split_expanded_flag<'a>(
        &'a self,
        flag: &'a UseFlag,
    ) -> Option<(&'a EnvVarName, &'a str)> {
        self.groups
            .iter()
            .filter_map(|(name, group)| match group {
                ExpandGroup::Prefixed { prefix } => flag
                    .as_str()
                    .strip_prefix(prefix.as_ref())
                    .map(|value| (name, value)),
                ExpandGroup::Unprefixed { .. } => None,
            })
            .min_by_key(|(_, value)| value.len())
    }

    /// Resolves a USE group value into its corresponding USE flag.
    pub fn resolve_flag(&self, group: &EnvVarName, value: &UseFlag) -> anyhow::Result<UseFlag> {
        match self.groups.get(group) {
            Some(declared) => declared.flag(value.as_str()),
            None => bail!("unknown USE expansion group '{group}'"),
        }
    }

    /// Returns whether `flag` belongs to the given USE expansion `group`.
    pub fn group_matches_flag(&self, group: &EnvVarName, flag: &UseFlag) -> bool {
        match self.groups.get(group) {
            Some(ExpandGroup::Prefixed { .. }) => self
                .split_expanded_flag(flag)
                .is_some_and(|(name, _)| name == group),
            Some(ExpandGroup::Unprefixed { values }) => values.contains(flag.as_str()),
            None => false,
        }
    }

    /// Returns the USE expand group names.
    pub fn names(&self) -> impl Iterator<Item = &EnvVarName> {
        self.groups.keys()
    }

    /// Materializes all groups into expanded desired USE assignments.
    pub fn materialize(&self, makenv: &MakeEnv) -> anyhow::Result<Vec<(UseFlag, bool)>> {
        let mut assignments = Vec::new();
        for (name, group) in &self.groups {
            let Some(value) = makenv.get(name.as_str()) else {
                continue;
            };
            let flags = expand_env_value(value, group)
                .with_context(|| format!("invalid USE expand value for {name}"))?;
            assignments.extend(flags);
        }
        Ok(assignments)
    }

    /// Returns USE flags injected into `IUSE_EFFECTIVE` by implicit expansion groups.
    pub fn implicit_flags(&self, makenv: &MakeEnv) -> anyhow::Result<FxHashSet<UseFlag>> {
        let mut flags = FxHashSet::default();

        for (name, group) in &self.groups {
            if !self.implicit_groups.contains(name) {
                continue;
            }

            let vars = format!("USE_EXPAND_VALUES_{name}");
            let Some(values) = makenv.get(vars.as_str()) else {
                continue;
            };

            for value in values.iter() {
                let flag = group
                    .flag(value)
                    .with_context(|| format!("invalid {vars}"))?;
                flags.insert(flag);
            }
        }
        Ok(flags)
    }
}

/// Parses a `USE_EXPAND` group list into validated names.
fn parse_groups(var: &str, values: Option<&EnvValue>) -> anyhow::Result<FxHashSet<EnvVarName>> {
    values
        .into_iter()
        .flat_map(EnvValue::iter)
        .map(|name| EnvVarName::new(name).with_context(|| format!("invalid {var} group")))
        .collect()
}

/// Expands the given [`EnvValue`] into USE flags for the given `group`.
fn expand_env_value(value: &EnvValue, group: &ExpandGroup) -> anyhow::Result<Vec<(UseFlag, bool)>> {
    let mut flags = Vec::default();
    for val in value.iter() {
        if val == "-*" {
            flags.clear();
            continue;
        }

        let (name, enabled) = match val.strip_prefix('-') {
            Some(value) => (value, false),
            None => (val, true),
        };
        flags.push((group.flag(name)?, enabled));
    }
    Ok(flags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::useflag::test_support::expand_config;

    #[test]
    fn test_split_prefixed_flags() -> anyhow::Result<()> {
        let config = expand_config(
            "USE_EXPAND=\"LLVM_TARGETS FOO FOO_BAR\"
             USE_EXPAND_UNPREFIXED=ARCH",
        )?;

        assert_eq!(
            config
                .split_expanded_flag(&"llvm_targets_LoongArch".parse()?)
                .map(|(grp, val)| (grp.as_str(), val)),
            Some(("LLVM_TARGETS", "LoongArch"))
        );
        assert_eq!(
            config
                .split_expanded_flag(&"foo_bar_baz".parse()?)
                .map(|(grp, val)| (grp.as_str(), val)),
            Some(("FOO_BAR", "baz"))
        );
        assert!(config.split_expanded_flag(&"amd64".parse()?).is_none());
        Ok(())
    }

    #[test]
    fn test_group_matches_flag() -> anyhow::Result<()> {
        let config = expand_config(
            r#"USE_EXPAND="VIDEO_CARDS VIDEO"
             USE_EXPAND_UNPREFIXED=ARCH
             USE_EXPAND_VALUES_ARCH=amd64"#,
        )?;

        assert!(config.group_matches_flag(
            &EnvVarName::new("VIDEO_CARDS")?,
            &UseFlag::new("video_cards_amdgpu")?
        ));
        assert!(!config.group_matches_flag(
            &EnvVarName::new("VIDEO")?,
            &UseFlag::new("video_cards_amdgpu")?
        ));
        assert!(config.group_matches_flag(&EnvVarName::new("ARCH")?, &UseFlag::new("amd64")?));
        assert!(!config.group_matches_flag(
            &EnvVarName::new("ARCH")?,
            &UseFlag::new("video_cards_amdgpu")?
        ));
        Ok(())
    }

    #[test]
    fn test_unknown_group() -> anyhow::Result<()> {
        let config = expand_config("USE_EXPAND=VIDEO_CARDS")?;
        let group = EnvVarName::new("ARCH")?;
        let flag = UseFlag::new("amd64")?;

        assert!(!config.group_matches_flag(&group, &flag));
        assert!(config.resolve_flag(&group, &flag).is_err());
        Ok(())
    }

    #[test]
    fn test_prefixed_values_not_unprefixed() -> anyhow::Result<()> {
        let config = expand_config(
            "USE_EXPAND=ARCH
             USE_EXPAND_VALUES_ARCH=amd64",
        )?;

        assert!(!config.group_matches_flag(&EnvVarName::new("ARCH")?, &UseFlag::new("amd64")?));
        assert!(config.group_matches_flag(&EnvVarName::new("ARCH")?, &UseFlag::new("arch_amd64")?));
        Ok(())
    }

    #[test]
    fn test_duplicate_groups() -> anyhow::Result<()> {
        let config = expand_config(
            "USE_EXPAND=\"VIDEO_CARDS VIDEO_CARDS\"
             USE_EXPAND_UNPREFIXED=\"ARCH ARCH\"",
        )?;

        assert_eq!(
            config
                .names()
                .map(EnvVarName::as_str)
                .collect::<FxHashSet<_>>(),
            FxHashSet::from_iter(["VIDEO_CARDS", "ARCH"])
        );
        Ok(())
    }

    #[test]
    fn test_materialize() -> anyhow::Result<()> {
        let makenv = MakeEnv::from_content(
            "USE_EXPAND=\"VIDEO_CARDS INPUT_DEVICES\"
            USE_EXPAND_UNPREFIXED=ARCH
            VIDEO_CARDS=\"amdgpu -amdgpu -* nouveau\"
            ARCH=\"amd64 -arm64\"",
        )?;
        let config = UseExpandConfig::from_makenv(&makenv)?;

        let result = config.materialize(&makenv)?;
        assert_eq!(
            result.into_iter().collect::<FxHashMap<_, _>>(),
            FxHashMap::from_iter([
                (UseFlag::new("video_cards_nouveau")?, true),
                (UseFlag::new("amd64")?, true),
                (UseFlag::new("arm64")?, false),
            ])
        );
        Ok(())
    }

    #[test]
    fn test_implicit_flags() -> anyhow::Result<()> {
        let makenv = MakeEnv::from_content(
            "USE_EXPAND=\"ELIBC VIDEO_CARDS INPUT_DEVICES\"
             USE_EXPAND_UNPREFIXED=ARCH
             USE_EXPAND_IMPLICIT=\"ARCH ELIBC INPUT_DEVICES\"
             USE_EXPAND_VALUES_ARCH=\"amd64 x86\"
             USE_EXPAND_VALUES_ELIBC=\"glibc\"
             USE_EXPAND_VALUES_VIDEO_CARDS=amdgpu",
        )?;
        let config = UseExpandConfig::from_makenv(&makenv)?;

        assert_eq!(
            config.implicit_flags(&makenv)?,
            FxHashSet::from_iter([
                UseFlag::new("amd64")?,
                UseFlag::new("x86")?,
                UseFlag::new("elibc_glibc")?,
            ])
        );
        Ok(())
    }

    #[test]
    fn test_rejects_overlapping_groups() {
        assert!(
            expand_config(
                "USE_EXPAND=\"LLVM_TARGETS\"
             USE_EXPAND_UNPREFIXED=\"LLVM_TARGETS\""
            )
            .is_err()
        );
    }
}
