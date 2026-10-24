use anyhow::bail;
use germ_pms::UseFlag;

use crate::types::{FxHashMap, FxHashSet};

/// Final USE state of one package.
///
/// It contains all flags defined by `IUSE_EFFECTIVE`
/// and their enabled state defined by `USE`.
#[derive(Debug, Clone, Eq, PartialEq)]
#[cfg_attr(test, derive(Default))]
pub struct EffectiveUse {
    /// Contains all defined flags mapped to their enabled state.
    flags: FxHashMap<UseFlag, bool>,
}

impl EffectiveUse {
    /// Creates a new `EffectiveUse` with the given `flags`.
    pub const fn new(flags: FxHashMap<UseFlag, bool>) -> Self {
        Self { flags }
    }

    /// Creates a new `EffectiveUse` from the given `available` and `enabled` flags.
    pub fn from_parts(
        available: FxHashSet<UseFlag>,
        enabled: FxHashSet<UseFlag>,
    ) -> anyhow::Result<Self> {
        let mut flags = available
            .into_iter()
            .map(|flag| (flag, false))
            .collect::<FxHashMap<_, _>>();
        for flag in enabled {
            match flags.get_mut(&flag) {
                Some(state) => *state = true,
                None => bail!("flag {flag} is not defined in IUSE_EFFECTIVE"),
            }
        }
        Ok(Self::new(flags))
    }

    /// Returns `true` if the given `flag` is enabled, `false` otherwise.
    /// `Err` is returned if the flag is not available.
    pub fn is_enabled(&self, flag: &UseFlag) -> anyhow::Result<bool> {
        match self.state(flag) {
            Some(enabled) => Ok(enabled),
            None => bail!("flag {flag} is not defined in IUSE_EFFECTIVE"),
        }
    }

    /// Returns `true` if the given `flag` is disabled, `false` otherwise.
    /// `Err` is returned if the flag is not available.
    pub fn is_disabled(&self, flag: &UseFlag) -> anyhow::Result<bool> {
        Ok(!self.is_enabled(flag)?)
    }

    /// Returns the effective state of the given `flag`.
    /// - `Some(true)` if the flag is enabled.
    /// - `Some(false)` if the flag is disabled.
    /// - `None` if the flag is not available.
    pub fn state(&self, flag: &UseFlag) -> Option<bool> {
        self.flags.get(flag).copied()
    }

    /// Returns iterator over all enabled flags.
    pub fn enabled(&self) -> impl Iterator<Item = &UseFlag> {
        self.flags
            .iter()
            .filter_map(|(flag, enabled)| enabled.then_some(flag))
    }

    /// Returns the state changes from `from` to `self`.
    pub fn changes<'a>(&'a self, from: &'a Self) -> impl Iterator<Item = (&'a UseFlag, bool)> {
        let enabled = self
            .enabled()
            .filter(move |flag| from.state(flag) != Some(true))
            .map(|flag| (flag, true));
        let disabled = from
            .enabled()
            .filter(move |flag| self.state(flag) != Some(true))
            .map(|flag| (flag, false));

        enabled.chain(disabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::useflag::test_support::effective;

    #[test]
    fn test_enabled() -> anyhow::Result<()> {
        let foo = UseFlag::new("foo")?;
        let bar = UseFlag::new("bar")?;
        let state = EffectiveUse::from_parts(
            FxHashSet::from_iter([foo, bar.clone()]),
            FxHashSet::from_iter([bar.clone()]),
        )?;

        assert_eq!(state.enabled().collect::<Vec<_>>(), [&bar]);
        Ok(())
    }

    #[test]
    fn test_from_parts_unknown_enabled() -> anyhow::Result<()> {
        let result = EffectiveUse::from_parts(
            FxHashSet::from_iter([UseFlag::new("foo")?]),
            FxHashSet::from_iter([UseFlag::new("bar")?]),
        );

        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_changes() -> anyhow::Result<()> {
        let old = effective(&["foo", "bar", "baz"], &["foo"]);
        let new = effective(&["bar", "baz", "qux"], &["baz", "qux"]);
        let changes = new.changes(&old).collect::<Vec<_>>();

        let foo = UseFlag::new("foo")?;
        let baz = UseFlag::new("baz")?;
        let qux = UseFlag::new("qux")?;
        assert_eq!(changes.len(), 3);
        assert!(changes.contains(&(&foo, false)));
        assert!(changes.contains(&(&baz, true)));
        assert!(changes.contains(&(&qux, true)));
        Ok(())
    }
}
