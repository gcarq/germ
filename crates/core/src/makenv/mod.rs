mod name;
mod stack;
mod value;

use crate::files::content_from_path;
use crate::types::{FxHashMap, FxHashSet};
use crate::useflag::UseExpandConfig;
use crate::utils::{self, Inherit};
pub use name::EnvVarName;
pub use stack::MakeEnvStack;
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
struct IncrementalVars(FxHashSet<EnvVarName>);

impl IncrementalVars {
    /// Collects incremental vars from effective USE expansion groups in `layers`.
    fn from_layers(layers: &[&MakeEnv]) -> anyhow::Result<Self> {
        let provisional = MakeEnv::fold(layers, &Self::default())?;
        let expand = UseExpandConfig::from_makenv(&provisional)?;
        Ok(Self(expand.names().cloned().collect()))
    }

    /// Returns `true` if the given `name` is an incremental variable.
    fn contains(&self, name: &EnvVarName) -> bool {
        INCREMENTAL_VARS.contains(&name.as_str()) || self.0.contains(name)
    }
}

/// Holds the make variables of one or multiple folded make files.
#[derive(Default, Clone)]
pub struct MakeEnv {
    vars: FxHashMap<EnvVarName, EnvValue>,
}

impl MakeEnv {
    pub fn from_path(path: &Path, recursive: bool, optional: bool) -> anyhow::Result<Self> {
        Self::from_content(&content_from_path(path, recursive, optional)?)
    }

    /// Builds a [`MakeEnv`] from the given content of a make.conf or make.defaults file.
    pub fn from_content(content: &str) -> anyhow::Result<Self> {
        let mut vars = utils::shlex_split(content)?
            .into_iter()
            .map(|(key, value)| Ok((EnvVarName::new(key)?, EnvValue::new(value.as_str()))))
            .collect::<anyhow::Result<Vec<_>>>()?;

        for i in 0..vars.len() {
            vars[i].1 = vars[i].1.expand_with(|name| {
                vars[..i]
                    .iter()
                    .rev()
                    .find_map(|(candidate, value)| (name == candidate.as_str()).then_some(value))
            })?;
        }

        Ok(Self {
            vars: vars.into_iter().collect(),
        })
    }

    /// Returns the [`EnvValue`] for the given `name`.
    pub fn get(&self, name: &str) -> Option<&EnvValue> {
        self.vars.get(name)
    }

    /// Returns an iterator over the name and value of all make variables.
    pub fn vars(&self) -> impl Iterator<Item = (&EnvVarName, &EnvValue)> {
        self.vars.iter()
    }

    /// Folds ordered layers using their effective USE expansion namespace.
    pub(crate) fn fold_with_use_expand(layers: &[&Self]) -> anyhow::Result<Self> {
        let vars = IncrementalVars::from_layers(layers)?;
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
        for value in self.vars.values_mut() {
            *value = value.expand_with(|var| parent.get(var))?;
        }
        Ok(())
    }

    /// Inherits a parent environment using supplied incremental variables.
    fn inherit_vars(&mut self, parent: &MakeEnv, vars: &IncrementalVars) -> anyhow::Result<()> {
        self.expand_from(parent)?;

        for (name, parent_value) in parent.vars() {
            match self.vars.get_mut(name) {
                Some(value) if vars.contains(name) => {
                    value.inherit(parent_value);
                }
                Some(_) => {}
                None => {
                    let mut value = parent_value.clone();
                    if vars.contains(name) {
                        value.normalize();
                    }
                    self.vars.insert(name.clone(), value);
                }
            }
        }

        for (name, value) in &mut self.vars {
            if vars.contains(name) && !parent.vars.contains_key(name) {
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
    fn test_makenv_vars() {
        let makenv = MakeEnv::from_content("ARCH=amd64 USE=foo").unwrap();
        let mut vars = makenv
            .vars()
            .map(|(name, value)| (name.as_str(), value.to_string()))
            .collect::<Vec<_>>();
        vars.sort();
        assert_eq!(
            vars,
            [("ARCH", "amd64".to_owned()), ("USE", "foo".to_owned())]
        );
    }

    #[test]
    fn test_makenv_expand_from() {
        let parent = MakeEnv::from_content("USE=foo").unwrap();
        let mut child = MakeEnv::from_content("USE=\"${USE} -foo\"").unwrap();
        child.expand_from(&parent).unwrap();

        assert_eq!(
            child.get("USE").map(ToString::to_string),
            Some("foo -foo".into())
        );
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
