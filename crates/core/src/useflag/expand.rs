use crate::makenv::{EnvValue, MakeEnv};
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::UseFlag;
use anyhow::{Context, bail};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UseExpandKind {
    Prefixed,
    Unprefixed,
}

/// Maps USE expansion groups to their expansion kind (prefixed or unprefixed).
#[derive(Clone, Debug, Default)]
pub struct UseExpandConfig {
    groups: FxHashMap<Box<str>, UseExpandKind>,
    /// Implicit groups get injected into `IUSE_IMPLICIT`.
    implicit_groups: FxHashSet<Box<str>>,
}

impl UseExpandConfig {
    /// Builds the expansion config from the effective [`MakeEnv`].
    pub fn from_makenv(makenv: &MakeEnv) -> anyhow::Result<Self> {
        let mut config = Self::default();
        config.add_groups(makenv.get("USE_EXPAND"), UseExpandKind::Prefixed)?;
        config.add_groups(
            makenv.get("USE_EXPAND_UNPREFIXED"),
            UseExpandKind::Unprefixed,
        )?;
        config.implicit_groups = makenv
            .get("USE_EXPAND_IMPLICIT")
            .into_iter()
            .flat_map(EnvValue::iter)
            .map(Into::into)
            .collect();
        Ok(config)
    }

    /// Resolves a USE group value into its corresponding USE flag.
    pub fn resolve_flag(&self, group: &str, value: &UseFlag) -> anyhow::Result<UseFlag> {
        match self.groups.get(group) {
            Some(kind) => expand_value(*kind, group, value.as_str()),
            None => bail!("unknown USE expansion group '{group}'"),
        }
    }

    /// Returns the USE expand group names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.groups.keys().map(AsRef::as_ref)
    }

    /// Materializes all groups into expanded desired USE assignments.
    pub fn materialize(&self, makenv: &MakeEnv) -> anyhow::Result<Vec<(UseFlag, bool)>> {
        let mut assignments = Vec::new();
        for (name, kind) in &self.groups {
            let Some(value) = makenv.get(name) else {
                continue;
            };
            let flags = expand_env_value(value, *kind, name)
                .with_context(|| format!("invalid USE expand value for {name}"))?;
            assignments.extend(flags);
        }
        Ok(assignments)
    }

    /// Returns USE flags injected into `IUSE_EFFECTIVE` by implicit expansion groups.
    pub fn implicit_flags(&self, makenv: &MakeEnv) -> anyhow::Result<FxHashSet<UseFlag>> {
        let mut flags = FxHashSet::default();

        for (name, kind) in &self.groups {
            if !self.implicit_groups.contains(name) {
                continue;
            }

            let vars = format!("USE_EXPAND_VALUES_{name}");
            let Some(values) = makenv.get(vars.as_str()) else {
                continue;
            };

            for value in values.iter() {
                let flag =
                    expand_value(*kind, name, value).with_context(|| format!("invalid {vars}"))?;
                flags.insert(flag);
            }
        }
        Ok(flags)
    }

    /// Adds the use expand groups from the given [`EnvValue`] to the config.
    fn add_groups(&mut self, values: Option<&EnvValue>, kind: UseExpandKind) -> anyhow::Result<()> {
        let Some(values) = values else {
            return Ok(());
        };

        for group in values.iter() {
            if let Some(existing) = self.groups.get(group) {
                if *existing != kind {
                    bail!("USE expansion group '{group}' is present in both USE_EXPAND namespaces");
                }
                continue;
            }
            self.groups.insert(group.into(), kind);
        }
        Ok(())
    }
}

/// Expands the given [`EnvValue`] into USE flags for the given `kind` and `group`.
fn expand_env_value(
    value: &EnvValue,
    kind: UseExpandKind,
    group: &str,
) -> anyhow::Result<Vec<(UseFlag, bool)>> {
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
        flags.push((expand_value(kind, group, name)?, enabled));
    }
    Ok(flags)
}

/// Expands one USE group value into its corresponding USE flag.
fn expand_value(kind: UseExpandKind, group: &str, value: &str) -> anyhow::Result<UseFlag> {
    match kind {
        UseExpandKind::Prefixed => UseFlag::new(format!("{}_{value}", group.to_ascii_lowercase())),
        UseExpandKind::Unprefixed => UseFlag::new(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_materialize() -> anyhow::Result<()> {
        let makenv = MakeEnv::from_content(
            "USE_EXPAND=VIDEO_CARDS
            USE_EXPAND_UNPREFIXED=ARCH
            VIDEO_CARDS=\"amdgpu -amdgpu -* nouveau\"
            ARCH=\"amd64 -arm64\"",
        )?;
        let config = UseExpandConfig::from_makenv(&makenv)?;

        assert_eq!(
            config
                .materialize(&makenv)?
                .into_iter()
                .collect::<FxHashMap<_, _>>(),
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
            "USE_EXPAND=\"ELIBC VIDEO_CARDS\"
             USE_EXPAND_UNPREFIXED=ARCH
             USE_EXPAND_IMPLICIT=\"ARCH ELIBC\"
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
    fn test_rejects_overlapping_groups() -> anyhow::Result<()> {
        let makenv = MakeEnv::from_content(
            "USE_EXPAND=\"LLVM_TARGETS\"
                USE_EXPAND_UNPREFIXED=\"LLVM_TARGETS\"",
        )?;

        assert!(UseExpandConfig::from_makenv(&makenv).is_err());
        Ok(())
    }
}
