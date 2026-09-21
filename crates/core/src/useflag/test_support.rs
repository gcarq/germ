use super::EffectiveUse;

/// Creates an [`EffectiveUse`] from available and enabled USE flags.
pub fn effective(available: &[&str], enabled: &[&str]) -> EffectiveUse {
    let available = available.iter().map(|flag| flag.parse().unwrap()).collect();
    let enabled = enabled.iter().map(|flag| flag.parse().unwrap()).collect();
    EffectiveUse::from_parts(available, enabled)
}
