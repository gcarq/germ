mod value;

use crate::files::content_from_path;
use crate::types::{FxHashMap, FxHashSet};
use crate::utils;
use crate::utils::Inherit;
use anyhow::{Context, bail};
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
pub(crate) struct IncrementalVars {
    vars: FxHashSet<Box<str>>,
}

impl IncrementalVars {
    /// Builds a classification from dynamic incremental variable values.
    pub(crate) fn from<'a>(values: impl IntoIterator<Item = &'a str>) -> Self {
        let mut vars = FxHashSet::default();
        for value in values {
            let mut normalized = EnvValue::default();
            normalized.inherit(&EnvValue::new(value));
            vars.extend(normalized.into_inner());
        }
        Self { vars }
    }

    /// Returns `true` if the given `name` is an incremental variable.
    fn contains(&self, name: &str) -> bool {
        INCREMENTAL_VARS.contains(&name) || self.vars.contains(name)
    }
}

/// Holds all environment variables defined in a make.conf or make.defaults file.
#[derive(Default, Clone)]
pub struct MakeEnv(FxHashMap<Box<str>, EnvValue>);

impl MakeEnv {
    pub fn from_path(path: &Path, recursive: bool, optional: bool) -> anyhow::Result<Self> {
        Self::from_string(content_from_path(path, recursive, optional)?)
    }

    /// Builds a [`MakeEnv`] from the given content of a make.conf or make.defaults file.
    pub fn from_string(content: String) -> anyhow::Result<Self> {
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

    /// Folds the given `layers` in order, passed `vars` are treated incremental.
    pub(crate) fn fold(layers: &[&MakeEnv], vars: &IncrementalVars) -> anyhow::Result<MakeEnv> {
        layers.iter().try_fold(MakeEnv::default(), |folded, layer| {
            let mut child = (*layer).clone();
            child.inherit_vars(&folded, vars)?;
            Ok(child)
        })
    }

    /// Inherits a parent environment using supplied incremental variables.
    pub(super) fn inherit_vars(
        &mut self,
        parent: &MakeEnv,
        vars: &IncrementalVars,
    ) -> anyhow::Result<()> {
        for value in self.0.values_mut() {
            *value = value.expand_with(|var| parent.get(var))?;
        }

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
    fn test_makenv_from_string_ok() {
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
        let makenv = MakeEnv::from_string(content.into()).unwrap();
        assert_eq!(makenv.get("USE").unwrap().to_string(), "cet -foo -bar");
        assert_eq!(
            makenv.get("BOOTSTRAP_USE").unwrap().to_string(),
            "${BOOTSTRAP_USE} cet"
        );
        assert_eq!(makenv.get("enable_year2038").unwrap().to_string(), "no");
    }

    #[test]
    fn test_makenv_from_string_err() {
        assert!(MakeEnv::from_string("/VAR1=test".into()).is_err());
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
        let parent = MakeEnv::from_string(parent_content.into()).unwrap();
        let mut child = MakeEnv::from_string(child_content.into()).unwrap();
        child.inherit_from(&parent).unwrap();
        assert_eq!(child.get("USE").unwrap().to_string(), "seccomp branding");
        assert_eq!(child.get("INPUT_DEVICES").unwrap().to_string(), "libinput");
        assert_eq!(child.get("GRUB_PLATFORM").unwrap().to_string(), "efi-64");
    }
}
