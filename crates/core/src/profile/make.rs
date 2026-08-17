use std::iter;

use super::Profile;
use crate::makenv::{IncrementalVars, MakeEnv};

const USE_EXPAND_VARS: [&str; 2] = ["USE_EXPAND", "USE_EXPAND_UNPREFIXED"];

/// Folds profile `make.defaults` layers into a [`MakeEnv`] and returns it.
pub fn fold_defaults(parents: &[Profile], profile: &Profile) -> anyhow::Result<MakeEnv> {
    let layers: Vec<&MakeEnv> = parents
        .iter()
        .map(|p| &p.make_defaults)
        .chain(iter::once(&profile.make_defaults))
        .collect();

    let provisional = fold_layers(&layers, &IncrementalVars::default())?;
    let vars = IncrementalVars::from(
        USE_EXPAND_VARS
            .into_iter()
            .filter_map(|var| provisional.get(var).map(ToString::to_string)),
    );
    fold_layers(&layers, &vars)
}

/// Folds the given `layers` into a single [`MakeEnv`] and returns it.
fn fold_layers(layers: &[&MakeEnv], vars: &IncrementalVars) -> anyhow::Result<MakeEnv> {
    layers.iter().try_fold(MakeEnv::default(), |folded, layer| {
        let mut child = (*layer).clone();
        child.inherit_vars(&folded, vars)?;
        Ok(child)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fold_profile_contents(contents: &[&str]) -> anyhow::Result<MakeEnv> {
        let profiles = contents
            .iter()
            .map(|content| {
                Ok(Profile {
                    make_defaults: MakeEnv::from_string((*content).to_owned())?,
                    ..Default::default()
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let Some((profile, parents)) = profiles.split_last() else {
            return Ok(MakeEnv::default());
        };

        fold_defaults(parents, profile)
    }

    #[test]
    fn test_fold_profile_incremental_reset() {
        let env = fold_profile_contents(&[
            "USE_EXPAND=\"CAMERAS ROOT\"\nROOT=\"-* root\"",
            "CAMERAS=\"-* ptp2\"",
        ])
        .unwrap();
        assert_eq!(env.get("CAMERAS").unwrap().to_string(), "ptp2");
        assert_eq!(env.get("ROOT").unwrap().to_string(), "root");
    }

    #[test]
    fn test_fold_profile_unprefixed() {
        let env = fold_profile_contents(&[
            "USE_EXPAND_UNPREFIXED=\"ARCH\"\nARCH=\"amd64 x86\"",
            "ARCH=\"-x86 arm64\"",
        ])
        .unwrap();
        assert_eq!(env.get("ARCH").unwrap().to_string(), "amd64 arm64");
    }

    #[test]
    fn test_fold_profile_multiple_members() {
        let env = fold_profile_contents(&[
            "USE_EXPAND=\"CAMERAS VIDEO_CARDS\"\nCAMERAS=\"canon\"\nVIDEO_CARDS=\"amdgpu\"",
            "CAMERAS=\"ptp2\"\nVIDEO_CARDS=\"-amdgpu radeonsi\"",
        ])
        .unwrap();
        assert_eq!(env.get("CAMERAS").unwrap().to_string(), "canon ptp2");
        assert_eq!(env.get("VIDEO_CARDS").unwrap().to_string(), "radeonsi");
    }

    #[test]
    fn test_fold_profile_name_removal() {
        let env = fold_profile_contents(&[
            "USE_EXPAND=\"CAMERAS VIDEO_CARDS\"\nCAMERAS=\"canon\"\nVIDEO_CARDS=\"amdgpu\"",
            "USE_EXPAND=\"-CAMERAS PYTHON_TARGETS\"\nCAMERAS=\"-canon nikon\"\nPYTHON_TARGETS=\"python3_12\"",
        ])
        .unwrap();
        assert_eq!(env.get("CAMERAS").unwrap().to_string(), "-canon nikon");
        assert_eq!(env.get("VIDEO_CARDS").unwrap().to_string(), "amdgpu");
        assert_eq!(env.get("PYTHON_TARGETS").unwrap().to_string(), "python3_12");
    }

    #[test]
    fn test_fold_profile_control_reset() {
        let env = fold_profile_contents(&[
            "USE_EXPAND=\"CAMERAS -* VIDEO_CARDS\"\nCAMERAS=\"canon\"\nVIDEO_CARDS=\"amdgpu\"",
            "CAMERAS=\"-canon nikon\"\nVIDEO_CARDS=\"-amdgpu radeonsi\"",
        ])
        .unwrap();
        assert_eq!(env.get("CAMERAS").unwrap().to_string(), "-canon nikon");
        assert_eq!(env.get("VIDEO_CARDS").unwrap().to_string(), "radeonsi");
    }

    #[test]
    fn test_fold_profile_name_readdition() {
        let env = fold_profile_contents(&[
            "USE_EXPAND=\"CAMERAS\"\nCAMERAS=\"canon\"",
            "USE_EXPAND=\"-CAMERAS\"\nCAMERAS=\"-canon nikon\"",
            "USE_EXPAND=\"CAMERAS\"\nCAMERAS=\"ptp2\"",
        ])
        .unwrap();
        assert_eq!(env.get("CAMERAS").unwrap().to_string(), "nikon ptp2");
    }

    #[test]
    fn test_fold_profile_context_expansion() {
        let env = fold_profile_contents(&[
            "MEMBER_NAMES=\"CAMERAS\"\nCAMERAS=\"canon\"",
            "USE_EXPAND=\"${MEMBER_NAMES}\"\nCAMERAS=\"-canon nikon\"",
        ])
        .unwrap();
        assert_eq!(env.get("CAMERAS").unwrap().to_string(), "nikon");
    }

    #[test]
    fn test_fold_profile_literal_replacement() {
        let env = fold_profile_contents(&[
            "INPUT_DEVICES=\"libinput\"",
            "INPUT_DEVICES=\"-libinput custom\"",
        ])
        .unwrap();
        assert_eq!(
            env.get("INPUT_DEVICES").unwrap().to_string(),
            "-libinput custom"
        );
    }
}
