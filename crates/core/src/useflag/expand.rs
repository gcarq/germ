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
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.groups.keys().map(AsRef::as_ref)
    }

    /// Materializes all groups into expanded USE flags.
    pub(crate) fn materialize(&self, makenv: &MakeEnv) -> anyhow::Result<Vec<UseFlag>> {
        let mut flags = Vec::new();
        for (group, kind) in &self.groups {
            let Some(values) = makenv.get(group.as_ref()) else {
                continue;
            };

            let prefix =
                matches!(kind, UseExpandKind::Prefixed).then(|| group.to_ascii_lowercase());

            for value in values.inner() {
                let flag = match &prefix {
                    Some(prefix) => UseFlag::new(format!("{prefix}_{value}")),
                    None => UseFlag::new(value.clone()),
                }
                .with_context(|| format!("invalid USE expand value for {group}"))?;
                flags.push(flag);
            }
        }
        Ok(flags)
    }

    /// Adds the use expand groups from the given [`EnvValue`] to the config.
    fn add_groups(&mut self, values: Option<&EnvValue>, kind: UseExpandKind) -> anyhow::Result<()> {
        let Some(values) = values else {
            return Ok(());
        };

        for group in values.inner() {
            if let Some(existing) = self.groups.get(group.as_ref()) {
                if *existing != kind {
                    bail!("USE expansion group '{group}' is present in both USE_EXPAND namespaces");
                }
                continue;
            }
            self.groups.insert(group.clone(), kind);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::entry::Precedence;

    #[test]
    fn test_from_makenv() -> anyhow::Result<()> {
        let makenv = MakeEnv::from_string(
            "USE_EXPAND=\"LLVM_TARGETS\"
                USE_EXPAND_UNPREFIXED=\"ARCH\""
                .into(),
        )?;
        let config = UseExpandConfig::from_makenv(&makenv)?;

        assert_eq!(
            config.groups.get("LLVM_TARGETS"),
            Some(&UseExpandKind::Prefixed)
        );
        assert_eq!(config.groups.get("ARCH"), Some(&UseExpandKind::Unprefixed));
        Ok(())
    }

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
