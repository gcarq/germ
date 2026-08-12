mod value;

use crate::files::content_from_path;
use crate::types::FxHashMap;
use crate::utils;
use crate::utils::Inherit;
use anyhow::{Context, Result, bail};
use std::ops::Deref;
use std::path::Path;
use value::EnvValue;

/// List of variables that are incremental as per PMS section 5.3 and
/// <https://github.com/gentoo/portage/blob/0783d820e6eecffa3adff52c4669fc715d65dbaa/lib/portage/const.py#L121>
/// NOTE: This list must be kept sorted for [`core::slice::binary_search`] to work correctly.
const INCREMENTAL_VARIABLES: [&str; 14] = [
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

/// Returns true if the given variable is incremental, false otherwise.
fn is_incremental_var(var: &str) -> bool {
    INCREMENTAL_VARIABLES.binary_search(&var).is_ok()
}

/// Holds all environment variables defined in a make.conf or make.defaults file.
#[derive(Default, Clone)]
pub struct MakeEnv(FxHashMap<String, EnvValue>);

impl MakeEnv {
    pub fn from_path(path: &Path, recursive: bool, optional: bool) -> Result<Self> {
        let content = content_from_path(path, recursive, optional)?;
        Self::from_string(content)
    }

    /// Builds a [`MakeEnv`] from the given content of a make.conf or make.defaults file.
    pub fn from_string(content: String) -> Result<Self> {
        let mut vars = utils::shlex_split(content)?
            .into_iter()
            .map(|(key, value)| {
                if key
                    .as_bytes()
                    .first()
                    .with_context(|| "variable name cannot be empty")?
                    .is_ascii_alphabetic()
                {
                    let value = EnvValue::new(value, is_incremental_var(&key));
                    Ok((key, value))
                } else {
                    bail!("invalid variable name: {key}")
                }
            })
            .collect::<Result<Vec<_>>>()?;

        for i in 0..vars.len() {
            vars[i].1 = vars[i].1.expand(&vars[..i])?;
        }

        Ok(Self(vars.into_iter().collect()))
    }

    /// Consumes self and returns the inner map.
    pub fn into_inner(self) -> FxHashMap<String, EnvValue> {
        self.0
    }
}

impl Inherit for MakeEnv {
    fn inherit_from(&mut self, parent: &MakeEnv) -> anyhow::Result<()> {
        let parent_ctx = parent
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<_>>();
        for (key, parent_value) in parent.iter() {
            // If the variable exists, expand it with the parent's context and take care of
            // incremental variables, otherwise just insert it.
            match self.0.get_mut(key) {
                Some(self_value) => {
                    *self_value = self_value.expand(&parent_ctx)?;
                    self_value.inherit_from(parent_value)?;
                }
                None => {
                    self.0.insert(key.clone(), parent_value.clone());
                }
            }
        }
        Ok(())
    }
}

impl Deref for MakeEnv {
    type Target = FxHashMap<String, EnvValue>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_env_from_string_ok() {
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
        let make_env = MakeEnv::from_string(content.into()).unwrap();
        assert_eq!(make_env.get("USE").unwrap().to_string(), "cet -foo -bar");
        assert_eq!(
            make_env.get("BOOTSTRAP_USE").unwrap().to_string(),
            "${BOOTSTRAP_USE} cet"
        );
        assert_eq!(make_env.get("enable_year2038").unwrap().to_string(), "no");
    }

    #[test]
    fn test_make_env_from_string_err() {
        assert!(MakeEnv::from_string("/VAR1=test".into()).is_err());
    }

    #[test]
    fn test_make_env_inherit_from() {
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
