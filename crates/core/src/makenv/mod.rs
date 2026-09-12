mod stack;
mod value;

use crate::files::content_from_path;
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::UseExpandConfig;
use crate::utils::{self, Inherit};
use anyhow::{Context, bail};
pub use stack::MakeEnvStack;
use std::ops::Deref;
use std::path::Path;
pub use value::EnvValue;

/// List of variables that are incremental as per PMS section 5.3 and
/// <https://github.com/gentoo/portage/blob/0783d820e6eecffa3adff52c4669fc715d65dbaa/lib/portage/const.py#L121>
const INCREMENTAL_VARS: [&str; 14] = [
    "ACCEPT_KEYWORDS",
    "CONFIG_PROTECT",
    "CONFIG_PROTECT_MASK",
    "ENV_UNSET",
    "FEATURES",
    "IUSE_IMPLICIT",
    "PRELINK_PATH",
    "PRELINK_PATH_MASK",
    "PROFILE_ONLY_VARIABLES",
    "USE",
    "USE_EXPAND",
    "USE_EXPAND_HIDDEN",
    "USE_EXPAND_IMPLICIT",
    "USE_EXPAND_UNPREFIXED",
];

/// Holds all variable names that must be considered incremental.
#[derive(Default)]
struct IncrementalVars(FxHashSet<Box<str>>);

impl IncrementalVars {
    /// Collects incremental vars from effective USE expansion groups in `layers`.
    pub fn from_makenv_layers(layers: &[&MakeEnv]) -> anyhow::Result<Self> {
        let provisional = MakeEnv::fold(layers, &Self::default())?;
        let expand = UseExpandConfig::from_makenv(&provisional)?;
        Ok(Self::from(expand.names()))
    }

    /// Collects the given incremental variable `names`.
    pub fn from<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        let mut vars = FxHashSet::default();
        for name in names {
            let mut value = EnvValue::new(name);
            value.normalize();
            vars.extend(value);
        }
        Self(vars)
    }

    /// Returns `true` if the given `name` is an incremental variable.
    fn contains(&self, name: &str) -> bool {
        INCREMENTAL_VARS.contains(&name) || self.0.contains(name)
    }
}

/// Holds env vars from either one or multiple folded make files.
#[derive(Default, Clone)]
pub struct MakeEnv(FxHashMap<Box<str>, EnvValue>);

impl MakeEnv {
    pub fn from_path(path: &Path, recursive: bool, optional: bool) -> anyhow::Result<Self> {
        Self::from_content(&content_from_path(path, recursive, optional)?)
    }

    /// Builds a [`MakeEnv`] from the given content of a make.conf or make.defaults file.
    pub fn from_content(content: &str) -> anyhow::Result<Self> {
        let mut vars = utils::shlex_split(content)?
            .into_iter()
            .map(|(key, value)| {
                if key
                    .as_bytes()
                    .first()
                    .context("variable name cannot be empty")?
                    .is_ascii_alphabetic()
                {
                    Ok((key.into_boxed_str(), EnvValue::new(value.as_str())))
                } else {
                    bail!("invalid variable name: {key}")
                }
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        for i in 0..vars.len() {
            vars[i].1 = vars[i].1.expand(&vars[..i])?;
        }

        Ok(Self(vars.into_iter().collect()))
    }

    /// Consumes self and returns the inner map.
    pub fn into_inner(self) -> FxHashMap<Box<str>, EnvValue> {
        self.0
    }

    /// Folds ordered layers using their effective USE expansion namespace.
    pub(crate) fn fold_with_use_expand(layers: &[&Self]) -> anyhow::Result<Self> {
        let vars = IncrementalVars::from_makenv_layers(layers)?;
        Self::fold(layers, &vars)
    }

    /// Folds the given `layers` in order, passed `vars` are treated incremental.
    fn fold(layers: &[&Self], vars: &IncrementalVars) -> anyhow::Result<Self> {
        layers.iter().try_fold(MakeEnv::default(), |folded, layer| {
            let mut child = (*layer).clone();
            child.inherit_vars(&folded, vars)?;
            Ok(child)
        })
    }

    /// Expands local variable references using values from `parent`.
    fn expand_from(&mut self, parent: &MakeEnv) -> anyhow::Result<()> {
        for value in self.0.values_mut() {
            *value = value.expand_with(|var| parent.get(var))?;
        }
        Ok(())
    }

    /// Inherits a parent environment using supplied incremental variables.
    fn inherit_vars(&mut self, parent: &MakeEnv, vars: &IncrementalVars) -> anyhow::Result<()> {
        self.expand_from(parent)?;

        for (key, parent_value) in parent.iter() {
            match self.0.get_mut(key) {
                Some(value) if vars.contains(key) => {
                    value.inherit(parent_value);
                }
                Some(_) => {}
                None => {
                    let mut value = parent_value.clone();
                    if vars.contains(key) {
                        value.normalize();
                    }
                    self.0.insert(key.clone(), value);
                }
            }
        }

        for (key, value) in &mut self.0 {
            if vars.contains(key) && !parent.contains_key(key) {
                value.normalize();
            }
        }
        Ok(())
    }
}

impl Inherit for MakeEnv {
    fn inherit_from(&mut self, parent: &MakeEnv) -> anyhow::Result<()> {
        self.inherit_vars(parent, &IncrementalVars::default())
    }
}

impl Deref for MakeEnv {
    type Target = FxHashMap<Box<str>, EnvValue>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_makenv_from_content_ok() {
        let content = r#"
# This is a comment
USE="cet"

# Should result in BOOTSTRAP_USE="${BOOTSTRAP_USE} cet"
BOOTSTRAP_USE="${BOOTSTRAP_USE}
$USE"

# Should result in USE="cet -foo -bar"
USE="${USE} -foo"
USE="${USE} -bar"

enable_year2038="no"
        "#;
        let makenv = MakeEnv::from_content(content).unwrap();
        assert_eq!(makenv.get("USE").unwrap().to_string(), "cet -foo -bar");
        assert_eq!(
            makenv.get("BOOTSTRAP_USE").unwrap().to_string(),
            "${BOOTSTRAP_USE} cet"
        );
        assert_eq!(makenv.get("enable_year2038").unwrap().to_string(), "no");
    }

    #[test]
    fn test_makenv_from_content_err() {
        assert!(MakeEnv::from_content("/VAR1=test").is_err());
    }

    #[test]
    fn test_makenv_expand_from() -> anyhow::Result<()> {
        let parent = MakeEnv::from_content("USE=foo")?;
        let mut child = MakeEnv::from_content("USE=\"${USE} -foo\"")?;
        child.expand_from(&parent)?;

        assert_eq!(
            child.get("USE").map(ToString::to_string),
            Some("foo -foo".into())
        );
        Ok(())
    }

    #[test]
    fn test_makenv_inherit_from() {
        let parent_content = r#"
        USE="cet -iconv"
        INPUT_DEVICES="libinput"
        "#;
        let child_content = r#"
        USE="${USE} seccomp branding -cet"
        GRUB_PLATFORM="efi-64"
        "#;
        let parent = MakeEnv::from_content(parent_content).unwrap();
        let mut child = MakeEnv::from_content(child_content).unwrap();
        child.inherit_from(&parent).unwrap();
        assert_eq!(child.get("USE").unwrap().to_string(), "seccomp branding");
        assert_eq!(child.get("INPUT_DEVICES").unwrap().to_string(), "libinput");
        assert_eq!(child.get("GRUB_PLATFORM").unwrap().to_string(), "efi-64");
    }

    fn fold_contents(contents: &[&str]) -> anyhow::Result<MakeEnv> {
        let envs = contents
            .iter()
            .map(|content| MakeEnv::from_content(content))
            .collect::<anyhow::Result<Vec<_>>>()
            .unwrap();
        let layers = envs.iter().collect::<Vec<_>>();
        MakeEnv::fold_with_use_expand(&layers)
    }

    #[test]
    fn test_fold_with_use_expand_members() {
        let env = fold_contents(&[
            "USE_EXPAND='CAMERAS ROOT'
            USE_EXPAND_UNPREFIXED=ARCH
            CAMERAS=canon
            ROOT='-* root'
            ARCH='amd64 x86'",
            "CAMERAS='-canon ptp2'
            ROOT=desktop
            ARCH='-x86 arm64'",
        ])
        .unwrap();
        assert_eq!(env.get("CAMERAS").unwrap().to_string(), "ptp2");
        assert_eq!(env.get("ROOT").unwrap().to_string(), "root desktop");
        assert_eq!(env.get("ARCH").unwrap().to_string(), "amd64 arm64");
    }

    #[test]
    fn test_fold_with_use_expand_readdition() {
        let env = fold_contents(&[
            "USE_EXPAND=CAMERAS CAMERAS=canon",
            "USE_EXPAND=-CAMERAS CAMERAS='-canon nikon'",
            "USE_EXPAND=CAMERAS CAMERAS=ptp2",
        ])
        .unwrap();
        assert_eq!(env.get("CAMERAS").unwrap().to_string(), "nikon ptp2");
    }

    #[test]
    fn test_fold_with_use_expand_reference() {
        let env = fold_contents(&[
            "MEMBER_NAMES=CAMERAS CAMERAS=canon",
            "USE_EXPAND='${MEMBER_NAMES}' CAMERAS='-canon nikon'",
        ])
        .unwrap();
        assert_eq!(env.get("CAMERAS").unwrap().to_string(), "nikon");
    }

    #[test]
    fn test_fold_with_use_expand_overlap() {
        assert!(fold_contents(&["USE_EXPAND=FOO USE_EXPAND_UNPREFIXED=FOO"]).is_err());
    }
}
