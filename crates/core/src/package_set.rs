//! Package sets resolve set targets such as `@world` into concrete package atoms.
//!
//! Built-in sets derive from the resolved profile and world files; other names
//! refer to set files under `etc/portage/sets`.

use std::fmt;
use std::str::FromStr;

use anyhow::{Context, bail};
use germ_pms::Atom;
use log::warn;

use crate::SysConf;
use crate::conf::portage::PortageConf;
use crate::files::parse_from_path;
use crate::types::{FxHashSet, FxIndexMap, FxIndexSet};
use crate::utils::list_files;

const SYSTEM_SET: &str = "system";
const SELECTED_PACKAGES_SET: &str = "selected-packages";
const SELECTED_SETS_SET: &str = "selected-sets";
const SELECTED_SET: &str = "selected";
const WORLD_SET: &str = "world";

/// A package target, either a single atom or a package set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Target {
    Atom(Atom),
    Set(SetName),
}

impl FromStr for Target {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> anyhow::Result<Self> {
        match value.strip_prefix('@') {
            Some(name) => Ok(Self::Set(name.parse()?)),
            None => Ok(Self::Atom(value.parse()?)),
        }
    }
}

/// Identifies a package set by its name without the leading `@`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SetName(String);

impl FromStr for SetName {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> anyhow::Result<Self> {
        let valid = !value.is_empty()
            && value != "."
            && value != ".."
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+')
            });
        if !valid {
            bail!("invalid package set name: @{value}");
        }
        Ok(Self(value.to_owned()))
    }
}

impl fmt::Display for SetName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "@{}", self.0)
    }
}

/// The raw members of all built-in and static package sets.
type SetMembers = FxIndexMap<SetName, Vec<Target>>;

/// Package sets resolved from a configuration, keyed by [`SetName`].
pub struct PackageSets {
    members: SetMembers,
}

impl PackageSets {
    /// Builds all package sets from the given `conf` and `sysconf`.
    pub fn new(conf: &PortageConf, sysconf: &SysConf) -> anyhow::Result<Self> {
        let mut members = FxIndexMap::default();

        members.insert(
            SYSTEM_SET.parse()?,
            conf.system_set()?.finalize().map(Target::Atom).collect(),
        );
        members.insert(
            SELECTED_PACKAGES_SET.parse()?,
            parse_from_path(&sysconf.world_path(), false, true, |line| {
                Ok(Target::Atom(line.parse()?))
            })?,
        );
        members.insert(
            SELECTED_SETS_SET.parse()?,
            parse_from_path(&sysconf.world_sets_path(), false, true, |line| {
                match line.parse()? {
                    target @ Target::Set(_) => Ok(target),
                    Target::Atom(_) => bail!("expected a set, found {line}"),
                }
            })?,
        );
        members.insert(
            SELECTED_SET.parse()?,
            [SELECTED_PACKAGES_SET, SELECTED_SETS_SET]
                .map(|name| Target::Set(SetName(name.to_owned())))
                .into(),
        );
        members.insert(
            WORLD_SET.parse()?,
            [SELECTED_SET, SYSTEM_SET]
                .map(|name| Target::Set(SetName(name.to_owned())))
                .into(),
        );

        add_static_sets(sysconf, &mut members)?;
        Ok(Self { members })
    }

    /// Resolves `targets` into their atoms, preserving insertion order.
    pub fn resolve(&self, targets: &[Target]) -> anyhow::Result<Vec<Atom>> {
        let mut atoms = FxIndexSet::default();
        let mut resolved_sets = FxHashSet::default();
        self.resolve_targets(targets, &mut resolved_sets, &mut atoms)?;
        Ok(atoms.into_iter().collect())
    }

    /// Resolves `targets` and their members into `atoms`.
    fn resolve_targets(
        &self,
        targets: &[Target],
        resolved_sets: &mut FxHashSet<SetName>,
        atoms: &mut FxIndexSet<Atom>,
    ) -> anyhow::Result<()> {
        for target in targets {
            match target {
                Target::Atom(atom) => {
                    atoms.insert(atom.clone());
                }
                Target::Set(name) => {
                    if !resolved_sets.insert(name.clone()) {
                        continue;
                    }
                    let members = self
                        .members
                        .get(name)
                        .with_context(|| format!("unknown package set: {name}"))?;
                    self.resolve_targets(members, resolved_sets, atoms)?;
                }
            }
        }
        Ok(())
    }
}

/// Adds the package sets defined by files under `etc/portage/sets`.
fn add_static_sets(sysconf: &SysConf, members: &mut SetMembers) -> anyhow::Result<()> {
    let path = sysconf.portage_conf().join("sets");
    if !path.is_dir() {
        return Ok(());
    }

    for file in list_files(&path)? {
        let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Ok(name) = name.parse::<SetName>() else {
            continue;
        };
        if members.contains_key(&name) {
            warn!("Ignoring static package set {name}, which shadows a built-in set");
            continue;
        }
        let targets = parse_from_path(&file, false, false, str::parse)?;
        members.insert(name, targets);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conf::test_support::build_portage_conf;
    use crate::test_support::{assert_display_eq, build_sysconf, write_file};

    /// Builds a [`PackageSets`] over a fresh config root with the given `entries`
    /// that consist of a relative path and file content.
    fn build_sets(entries: &[(&str, &str)]) -> anyhow::Result<PackageSets> {
        let temp = tempfile::tempdir()?;
        let conf = build_portage_conf(&temp)?;
        let sysconf = build_sysconf(&temp);
        for (relative, contents) in entries {
            let path = match *relative {
                "world" => sysconf.world_path(),
                "world_sets" => sysconf.world_sets_path(),
                _ => sysconf.portage_conf().join(relative),
            };
            write_file(path, contents)?;
        }

        PackageSets::new(&conf, &sysconf)
    }

    #[test]
    fn test_targets() -> anyhow::Result<()> {
        assert_eq!(
            "dev-lang/rust".parse::<Target>()?,
            Target::Atom("dev-lang/rust".parse()?)
        );
        assert_eq!(
            ">=dev-lang/python-3.13".parse::<Target>()?,
            Target::Atom(">=dev-lang/python-3.13".parse()?)
        );
        assert_eq!("@world".parse::<Target>()?, Target::Set("world".parse()?));
        for invalid in ["@", "@../evil", "@bad name", "@a/b", "not-an-atom"] {
            assert!(invalid.parse::<Target>().is_err(), "{invalid}");
        }
        Ok(())
    }

    #[test]
    fn test_set_names() -> anyhow::Result<()> {
        for valid in ["world", "development", "my-set_1.0+build"] {
            assert_eq!(valid.parse::<SetName>()?.to_string(), format!("@{valid}"));
        }
        for invalid in ["", ".", "..", "a/b", "a b", "*"] {
            assert!(invalid.parse::<SetName>().is_err(), "{invalid}");
        }
        Ok(())
    }

    #[test]
    fn test_resolve_world() -> anyhow::Result<()> {
        let entries = [
            (
                "world",
                "dev-lang/python\n>=dev-lang/python-3.13\napp-editors/zed\n",
            ),
            ("world_sets", "@development\n"),
            ("sets/development", "dev-util/cmake\n@editors\n"),
            ("sets/editors", "app-editors/zed\napp-editors/neovim\n"),
            ("sets/empty", ""),
        ];

        let sets = build_sets(&entries)?;
        let cases: &[(&str, &[&str])] = &[
            (
                "world",
                &[
                    "dev-lang/python",
                    ">=dev-lang/python-3.13",
                    "app-editors/zed",
                    "dev-util/cmake",
                    "app-editors/neovim",
                    "sys-libs/glibc",
                ],
            ),
            (
                "selected",
                &[
                    "dev-lang/python",
                    ">=dev-lang/python-3.13",
                    "app-editors/zed",
                    "dev-util/cmake",
                    "app-editors/neovim",
                ],
            ),
            ("system", &["sys-libs/glibc"]),
            ("empty", &[]),
        ];
        for &(name, expected) in cases {
            let result = sets.resolve(&[Target::Set(name.parse()?)]);
            assert_display_eq(result?, expected);
        }
        Ok(())
    }

    #[test]
    fn test_builtin_world() -> anyhow::Result<()> {
        let cases: &[&[(&str, &str)]] = &[&[], &[("sets/world", "dev-lang/rust\n")]];

        for entries in cases {
            let sets = build_sets(entries)?;
            let result = sets.resolve(&[Target::Set("world".parse()?)]);
            assert_display_eq(result?, ["sys-libs/glibc"]);
        }
        Ok(())
    }

    #[test]
    fn test_build_errors() {
        for (path, contents) in [
            ("world", "not-an-atom\n"),
            ("world_sets", "dev-lang/rust\n"),
            ("sets/broken", "not-an-atom\n"),
        ] {
            assert!(build_sets(&[(path, contents)]).is_err(), "{path}");
        }
    }

    #[test]
    fn test_shared_sets() -> anyhow::Result<()> {
        let sets = build_sets(&[
            ("sets/top", "@left\n@right\n"),
            ("sets/left", "dev-lang/rust\n@right\n"),
            ("sets/right", "dev-lang/python\n@left\n"),
        ])?;
        let result = sets.resolve(&[Target::Set("top".parse()?)]);
        assert_display_eq(result?, ["dev-lang/rust", "dev-lang/python"]);
        Ok(())
    }

    #[test]
    fn test_resolve() -> anyhow::Result<()> {
        let sets = build_sets(&[
            ("sets/extra", "dev-lang/rust\ndev-lang/python\n"),
            ("sets/broken", "@missing\n"),
        ])?;
        let targets = ["dev-lang/rust", "@extra", "@extra", "dev-lang/rust"]
            .iter()
            .map(|target| target.parse::<Target>())
            .collect::<anyhow::Result<Vec<_>>>()?;
        assert_display_eq(
            sets.resolve(&targets)?,
            ["dev-lang/rust", "dev-lang/python"],
        );
        for name in ["missing", "broken", "preserved-rebuild"] {
            let result = sets.resolve(&[Target::Set(name.parse()?)]);
            assert!(result.is_err(), "{name}");
        }
        Ok(())
    }
}
