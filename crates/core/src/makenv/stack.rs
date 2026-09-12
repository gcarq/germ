use anyhow::Context;

use super::{EnvValue, IncrementalVars, MakeEnv};
use crate::keyword::KeywordSelector;
use crate::repository::Arch;
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::{UseExpandConfig, UseFlag};

/// Holds all make envs. They are split into global, profile,
/// and user layers, which are folded into a final make env.
pub struct MakeEnvStack {
    #[expect(dead_code)]
    global: MakeEnv,
    profile: MakeEnv,
    user: MakeEnv,
    // The final resolved make env after folding all layers.
    // TODO: this feels like a hack, but works for now,
    //       in an optimal case, `user` should be the "final" make env.
    makenv: MakeEnv,
}

impl MakeEnvStack {
    /// Expands global, profile, and user make config layers.
    pub fn new(global: MakeEnv, profile: MakeEnv, user: MakeEnv) -> anyhow::Result<Self> {
        let vars = IncrementalVars::from_makenv_layers(&[&global, &profile, &user])?;
        let inherited = MakeEnv::fold(&[&global, &profile], &vars)?;
        let mut user = user;
        user.expand_from(&inherited)?;
        let makenv = MakeEnv::fold(&[&global, &profile, &user], &vars)?;

        Ok(Self {
            global,
            profile,
            user,
            makenv,
        })
    }

    /// Returns the desired USE state from the resolved make env.
    pub fn global_use(&self) -> anyhow::Result<FxHashMap<UseFlag, bool>> {
        let expand_conf =
            UseExpandConfig::from_makenv(&self.makenv).context("unable to expand USE")?;
        let mut state = FxHashMap::default();

        apply_use_state(&mut state, self.makenv.get("USE")).context("invalid USE")?;
        apply_use_state(&mut state, self.user.get("USE")).context("invalid USE")?;

        for (flag, enabled) in expand_conf.materialize(&self.makenv)? {
            state.insert(flag, enabled);
        }

        let user_disabled = expand_conf
            .materialize(&self.user)?
            .into_iter()
            .filter(|(_, enabled)| !enabled);
        state.extend(user_disabled);

        Ok(state)
    }

    /// Returns the resolved `ARCH`.
    pub fn arch(&self) -> anyhow::Result<Arch> {
        self.makenv
            .get("ARCH")
            .context("ARCH is not set")?
            .to_string()
            .parse()
    }

    /// Returns the calculated `IUSE_EFFECTIVE` from profile defaults.
    ///
    /// This is constructed by `IUSE_IMPLICIT` and `USE_EXPAND_IMPLICIT`.
    pub fn iuse_implicit(&self) -> anyhow::Result<FxHashSet<UseFlag>> {
        let mut flags = match self.profile.get("IUSE_IMPLICIT") {
            Some(value) => value
                .iter()
                .map(str::parse)
                .collect::<anyhow::Result<_>>()
                .context("invalid IUSE_IMPLICIT")?,
            None => FxHashSet::default(),
        };
        let expand = UseExpandConfig::from_makenv(&self.profile)?;
        flags.extend(expand.implicit_flags(&self.profile)?);
        Ok(flags)
    }

    /// Returns the resolved `ACCEPT_KEYWORDS`.
    pub fn accept_keywords(&self) -> anyhow::Result<Vec<KeywordSelector>> {
        match self.makenv.get("ACCEPT_KEYWORDS") {
            Some(value) => value.iter().map(str::parse).collect(),
            None => Ok(Vec::new()),
        }
    }

    /// Returns the "final" resolved make env.
    pub const fn makenv(&self) -> &MakeEnv {
        &self.makenv
    }
}

/// Applies incremental USE state from a make variable.
fn apply_use_state(
    state: &mut FxHashMap<UseFlag, bool>,
    flags: Option<&EnvValue>,
) -> anyhow::Result<()> {
    let Some(flags) = flags else {
        return Ok(());
    };

    for flag in flags.iter() {
        if flag == "-*" {
            state.clear();
            continue;
        }
        let (name, enabled) = match flag.strip_prefix('-') {
            Some(value) => (value, false),
            None => (flag, true),
        };
        state.insert(UseFlag::new(name)?, enabled);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_use() -> anyhow::Result<()> {
        let stack = MakeEnvStack::new(
            MakeEnv::from_content("USE=foo")?,
            MakeEnv::from_content("USE=\"bar -foo\"")?,
            MakeEnv::default(),
        )?;
        assert_eq!(
            stack.global_use()?,
            FxHashMap::from_iter([(UseFlag::new("bar")?, true)])
        );

        let stack = MakeEnvStack::new(
            MakeEnv::from_content("USE=foo")?,
            MakeEnv::default(),
            MakeEnv::from_content("USE=-foo")?,
        )?;
        assert_eq!(
            stack.global_use()?,
            FxHashMap::from_iter([(UseFlag::new("foo")?, false)])
        );

        let stack = MakeEnvStack::new(
            MakeEnv::from_content("USE=\"foo bar\"")?,
            MakeEnv::default(),
            MakeEnv::from_content("USE=\"-* baz\"")?,
        )?;
        assert_eq!(
            stack.global_use()?,
            FxHashMap::from_iter([(UseFlag::new("baz")?, true)])
        );

        let stack = MakeEnvStack::new(
            MakeEnv::default(),
            MakeEnv::default(),
            MakeEnv::from_content("USE=+invalid")?,
        )?;
        assert!(stack.global_use().is_err());
        Ok(())
    }

    #[test]
    fn test_global_use_expansion() -> anyhow::Result<()> {
        let stack = MakeEnvStack::new(
            MakeEnv::from_content(
                "USE_EXPAND=VIDEO_CARDS
                 VIDEO_CARDS=amdgpu",
            )?,
            MakeEnv::from_content("VIDEO_CARDS=nouveau")?,
            MakeEnv::from_content("VIDEO_CARDS=\"-amdgpu radeonsi\"")?,
        )?;
        assert_eq!(
            stack.global_use()?,
            FxHashMap::from_iter([
                (UseFlag::new("video_cards_amdgpu")?, false),
                (UseFlag::new("video_cards_nouveau")?, true),
                (UseFlag::new("video_cards_radeonsi")?, true),
            ])
        );

        let stack = MakeEnvStack::new(
            MakeEnv::from_content(
                "USE_EXPAND=VIDEO_CARDS
                VIDEO_CARDS=amdgpu",
            )?,
            MakeEnv::default(),
            MakeEnv::from_content(
                "USE_EXPAND=\"-VIDEO_CARDS INPUT_DEVICES\"
                    INPUT_DEVICES=libinput",
            )?,
        )?;
        assert_eq!(
            stack.global_use()?,
            FxHashMap::from_iter([(UseFlag::new("input_devices_libinput")?, true)])
        );
        Ok(())
    }

    #[test]
    fn test_iuse_implicit_profile() -> anyhow::Result<()> {
        let stack = MakeEnvStack::new(
            MakeEnv::default(),
            MakeEnv::from_content(
                "IUSE_IMPLICIT=profile_flag
                 USE_EXPAND_UNPREFIXED=ARCH
                 USE_EXPAND_IMPLICIT=ARCH
                 USE_EXPAND_VALUES_ARCH=\"amd64\"",
            )?,
            MakeEnv::from_content("IUSE_IMPLICIT=local_flag")?,
        )?;

        let flags = FxHashSet::from_iter([UseFlag::new("profile_flag")?, UseFlag::new("amd64")?]);
        assert_eq!(stack.iuse_implicit()?, flags);
        Ok(())
    }
}
