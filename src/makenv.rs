use crate::conf::EnvValue;
use crate::profile::InheritFrom;
use crate::utils;
use crate::utils::FileFromPath;
use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;

/// List of variables that are incremental as per PMS section 5.3 and
/// https://github.com/gentoo/portage/blob/0783d820e6eecffa3adff52c4669fc715d65dbaa/lib/portage/const.py#L121
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

/// Holds all environment variables defined in a make.conf or make.defaults file.
#[derive(Debug, Default)]
pub struct MakeEnv {
    vars: HashMap<String, EnvValue>,
}

impl MakeEnv {
    pub fn get(&self, key: &str) -> Option<&EnvValue> {
        self.vars.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &EnvValue)> {
        self.vars.iter()
    }
}

impl FileFromPath for MakeEnv {
    /// Builds a [`MakeEnv`] from the given content of a make.conf or make.defaults file.
    fn from_file_content(content: String) -> Result<Self>
    where
        Self: Sized,
    {
        let mut vars = utils::shlex_split(content)?
            .into_iter()
            .map(|(key, value)| {
                if key
                    .chars()
                    .next()
                    .with_context(|| "variable name cannot be empty")?
                    .is_ascii_alphabetic()
                {
                    let is_incremental = INCREMENTAL_VARIABLES.binary_search(&key.as_str()).is_ok();
                    Ok((key, EnvValue::new(value, is_incremental)))
                } else {
                    Err(anyhow!("invalid variable name: {key}"))
                }
            })
            .collect::<Result<Vec<(String, EnvValue)>>>()?;

        for i in 0..vars.len() {
            vars[i].1 = vars[i].1.expand(&vars[..i]);
        }

        let vars = vars.into_iter().collect::<HashMap<String, EnvValue>>();
        Ok(Self { vars })
    }
}

impl InheritFrom for MakeEnv {
    fn inherit_from(&mut self, parent: &MakeEnv) {
        let parent_ctx = Vec::from_iter(parent.vars.iter().map(|(k, v)| (k.clone(), v.clone())));
        for (key, parent_value) in &parent.vars {
            // If the variable exists, expand it with the parent's context and take care of
            // incremental variables, otherwise just insert it.
            match self.vars.get_mut(key) {
                Some(self_value) => {
                    *self_value = self_value.expand(parent_ctx.as_slice());
                    if let EnvValue::Incremental(self_v) = self_value
                        && let EnvValue::Incremental(parent_v) = parent_value
                    {
                        let mut values = Vec::new();
                        for value in parent_v.iter().chain(self_v.iter()) {
                            if value == "-*" {
                                values.clear();
                                continue;
                            }
                            if value.starts_with('-') {
                                values.retain(|v| v != &value[1..]);
                                continue;
                            }
                            if !values.contains(value) {
                                values.push(value.clone());
                            }
                        }
                        values.sort_unstable();
                        *self_value = EnvValue::Incremental(values);
                    }
                }
                None => {
                    self.vars.insert(key.clone(), parent_value.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_file_content() {
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
        let make_env = MakeEnv::from_file_content(content.to_string()).unwrap();
        assert_eq!(make_env.get("USE").unwrap().to_string(), "cet -foo -bar");
        assert_eq!(
            make_env.get("BOOTSTRAP_USE").unwrap().to_string(),
            "${BOOTSTRAP_USE} cet"
        );
        assert_eq!(make_env.get("enable_year2038").unwrap().to_string(), "no");
    }

    #[test]
    fn test_make_env_inherit_from() {
        let parent_content = r#"
USE="cet -iconv"
        "#;
        let child_content = r#"
USE="${USE} seccomp branding -cet"
        "#;
        let parent = MakeEnv::from_file_content(parent_content.into()).unwrap();
        let mut child = MakeEnv::from_file_content(child_content.into()).unwrap();
        child.inherit_from(&parent);
        assert_eq!(child.get("USE").unwrap().to_string(), "branding seccomp");
    }
}
