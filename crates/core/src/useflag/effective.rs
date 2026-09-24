use crate::types::FxHashSet;
use anyhow::bail;

use super::UseFlag;

/// Final USE state of one package.
#[derive(Debug, Clone)]
#[cfg_attr(test, derive(Default))]
pub struct EffectiveUse {
    /// All flags that can exist for the package.
    /// This corresponds to `IUSE_EFFECTIVE`.
    available: FxHashSet<UseFlag>,
    /// Available flags that are enabled.
    enabled: FxHashSet<UseFlag>,
}

impl EffectiveUse {
    /// Returns `true` if the given `flag` is enabled, `false` otherwise.
    /// `Err` is returned if the flag is not available.
    pub fn is_enabled(&self, flag: &UseFlag) -> anyhow::Result<bool> {
        match self.state(flag) {
            Some(enabled) => Ok(enabled),
            None => bail!("flag {flag} is not available"),
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
        self.available
            .contains(flag)
            .then(|| self.enabled.contains(flag))
    }

    pub(crate) const fn from_parts(
        available: FxHashSet<UseFlag>,
        enabled: FxHashSet<UseFlag>,
    ) -> Self {
        Self { available, enabled }
    }
}
