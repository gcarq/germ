use super::{EffectiveUse, UseExpandConfig};
use crate::makenv::MakeEnv;

/// Creates an [`EffectiveUse`] from available and enabled USE flags.
pub fn effective(available: &[&str], enabled: &[&str]) -> EffectiveUse {
    let available = available.iter().map(|flag| flag.parse().unwrap()).collect();
    let enabled = enabled.iter().map(|flag| flag.parse().unwrap()).collect();
    EffectiveUse::from_parts(available, enabled).unwrap()
}

/// Builds a [`UseExpandConfig`] from the given make variable content.
pub fn expand_config(content: &str) -> anyhow::Result<UseExpandConfig> {
    UseExpandConfig::from_makenv(&MakeEnv::from_content(content)?)
}
