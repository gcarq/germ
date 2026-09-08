use crate::files::entry::{Entry, Operation};
use crate::makenv::{EnvValue, MakeEnv};
use crate::types::FxHashMap;
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
        Ok(config)
    }

    /// Expands the given use flag `entry` for the given `group`.
    pub fn expand_entry(
        &self,
        group: &str,
        entry: Entry<UseFlag>,
    ) -> anyhow::Result<Entry<UseFlag>> {
        let Some(kind) = self.groups.get(group) else {
            bail!("unknown USE expansion group '{group}'");
        };

        match kind {
            UseExpandKind::Unprefixed => Ok(entry),
            UseExpandKind::Prefixed => {
                let flag = entry.inner();
                let value = match entry.op {
                    Operation::Set => format!("{}_{flag}", group.to_ascii_lowercase()),
                    Operation::Unset => format!("-{}_{flag}", group.to_ascii_lowercase()),
                };
                Entry::from_str(&value, entry.prec)
            }
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
            let flags = expand_env_value(value, kind, name)
                .with_context(|| format!("invalid USE expand value for {name}"))?;
            assignments.extend(flags);
        }
        Ok(assignments)
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
    kind: &UseExpandKind,
    group: &str,
) -> anyhow::Result<Vec<(UseFlag, bool)>> {
    let prefix = matches!(kind, UseExpandKind::Prefixed).then(|| group.to_ascii_lowercase());

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
        let flag = match &prefix {
            Some(prefix) => UseFlag::new(format!("{prefix}_{name}"))?,
            None => UseFlag::new(name)?,
        };
        flags.push((flag, enabled));
    }
    Ok(flags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::entry::Precedence;

    #[test]
    fn test_expand_entry() -> anyhow::Result<()> {
        let makenv = MakeEnv::from_string(
            "USE_EXPAND=\"LLVM_TARGETS\"
                USE_EXPAND_UNPREFIXED=\"ARCH\""
                .into(),
        )?;
        let config = UseExpandConfig::from_makenv(&makenv)?;

        let expanded = config.expand_entry(
            "LLVM_TARGETS",
            Entry::from_str("-WebAssembly", Precedence::User)?,
        )?;
        assert_eq!(expanded.as_str(), "llvm_targets_WebAssembly");

        let expanded = config.expand_entry("ARCH", Entry::from_str("amd64", Precedence::User)?)?;
        assert_eq!(expanded.as_str(), "amd64");
        Ok(())
    }

    #[test]
    fn test_materialize() -> anyhow::Result<()> {
        let makenv = MakeEnv::from_string(
            "USE_EXPAND=VIDEO_CARDS
            USE_EXPAND_UNPREFIXED=ARCH
            VIDEO_CARDS=\"amdgpu -amdgpu -* nouveau\"
            ARCH=\"amd64 -arm64\""
                .into(),
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
    fn test_rejects_overlapping_groups() -> anyhow::Result<()> {
        let makenv = MakeEnv::from_string(
            "USE_EXPAND=\"LLVM_TARGETS\"
                USE_EXPAND_UNPREFIXED=\"LLVM_TARGETS\""
                .into(),
        )?;

        assert!(UseExpandConfig::from_makenv(&makenv).is_err());
        Ok(())
    }
}
