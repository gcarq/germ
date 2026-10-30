use std::borrow::Cow;
use std::path::Path;

use germ_pms::Atom;
use log::warn;

use crate::files::entry::{Entry, LineOrigin, Precedence};
use crate::files::{AtomEntries, parse_from_path};
use crate::utils::Inherit;

/// Holds the system package list of a profile `packages` file.
///
/// Only `*`-prefixed entries contribute; other lines are ignored.
#[derive(Clone, Debug, Default)]
pub struct SystemPackages(AtomEntries);

impl SystemPackages {
    /// Parses a profile `packages` file.
    pub fn from_path(path: &Path, order: Precedence) -> anyhow::Result<Self> {
        let mut system = Self::default();
        parse_from_path(path, false, true, |text, origin| {
            system.push(text, order, origin)
        })?;
        Ok(system)
    }

    /// Consumes self and returns the atoms that remain set.
    pub fn finalize(self) -> impl Iterator<Item = Atom> {
        self.0
            .into_entries()
            .filter(|entry| entry.op.as_bool())
            .map(Entry::into_inner)
    }

    /// Adds one `packages` entry.
    ///
    /// Only package entries with the format `*<atom>` and `-*` are considered.
    fn push(&mut self, line: &str, order: Precedence, origin: LineOrigin) -> anyhow::Result<()> {
        if line == "-*" {
            return self.0.push(line, order, origin);
        }

        let (negated, body) = match line.strip_prefix('-') {
            Some(body) => (true, body),
            None => (false, line),
        };
        match body.strip_prefix('*') {
            Some("") => warn!("Ignoring empty system package entry"),
            Some(atom) => {
                let entry = match negated {
                    true => Cow::Owned(format!("-{atom}")),
                    false => Cow::Borrowed(atom),
                };
                self.0.push(&entry, order, origin)?;
            }
            None => (),
        }
        Ok(())
    }
}

impl Inherit for SystemPackages {
    fn inherit_from(&mut self, parent: &Self) -> anyhow::Result<()> {
        self.0.inherit_from(&parent.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::test_support::with_origins;
    use crate::test_support::{assert_display_eq, write_file};

    fn parse(lines: &str) -> anyhow::Result<SystemPackages> {
        let mut system = SystemPackages::default();
        for (line, origin) in with_origins(lines.lines()) {
            system.push(line, Precedence::Profile(0), origin)?;
        }
        Ok(system)
    }

    #[test]
    fn test_system_set() -> anyhow::Result<()> {
        let cases: &[(&str, &[&str])] = &[
            ("*sys-libs/glibc\napp-misc/tool\n", &["sys-libs/glibc"]),
            ("-*\n*sys-apps/busybox\n", &["sys-apps/busybox"]),
            ("*sys-apps/sed\n*-sys-apps/sed\n", &[]),
            ("*sys-apps/sed\n-*sys-apps/sed\n", &[]),
            ("*\n", &[]),
            ("app-misc/tool\n", &[]),
        ];

        for &(lines, expected) in cases {
            assert_display_eq(parse(lines)?.finalize(), expected);
        }
        Ok(())
    }

    #[test]
    fn test_from_path() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;

        let missing =
            SystemPackages::from_path(&temp.path().join("packages"), Precedence::Profile(0))?;
        assert_display_eq(missing.finalize(), [] as [&str; 0]);

        let path = temp.path().join("packages");
        write_file(
            &path,
            "sys-libs/glibc # base\n\n*sys-apps/busybox\n\n*sys-apps/sed\n",
        )?;
        let entries = SystemPackages::from_path(&path, Precedence::Profile(0))?;
        assert_display_eq(entries.finalize(), ["sys-apps/busybox", "sys-apps/sed"]);

        Ok(())
    }
}
