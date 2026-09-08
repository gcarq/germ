mod keywords;
mod useflags;

pub(crate) use keywords::{KeywordRule, PackageAcceptKeywords};
pub(crate) use useflags::{PackageUseRecords, UseFlags};

use anyhow::Context;
use std::path::Path;

use crate::deps::atom::Atom;
use crate::files::{content_from_path, entry::Precedence};
use crate::utils::{Inherit, strip_line_comment};
use indexmap::IndexMap;

/// This trait abstracts multiple values in a line-based file, such as `package.use`.
pub trait AtomPolicy: Default + Inherit {
    fn parse(value: &str, precedence: Precedence) -> anyhow::Result<Self>;
    fn update_from(&mut self, other: Self);
}

/// Maps atoms to their policies while preserving insertion order.
#[derive(Clone, Debug, Default)]
pub struct AtomPolicies<T: AtomPolicy>(IndexMap<Atom, T>);

impl<T: AtomPolicy> AtomPolicies<T> {
    pub fn from_path(path: &Path, precedence: Precedence, recursive: bool) -> anyhow::Result<Self> {
        let content = content_from_path(path, recursive, true)?;
        Self::from_string(content, precedence)
            .with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn from_string(content: String, precedence: Precedence) -> anyhow::Result<Self> {
        let mut policies = IndexMap::<Atom, T>::default();

        for (lineno, line) in content.lines().enumerate() {
            let line = strip_line_comment(line);
            if line.is_empty() {
                continue;
            }

            let (atom, policy) = Self::parse_line(line, precedence)
                .with_context(|| format!("error in line {}: {line}", lineno + 1))?;
            policies.entry(atom).or_default().update_from(policy);
        }

        Ok(Self(policies))
    }

    pub fn into_iter(self) -> impl Iterator<Item = (Atom, T)> {
        self.0.into_iter()
    }

    fn parse_line(line: &str, precedence: Precedence) -> anyhow::Result<(Atom, T)> {
        let (atom, value) = match line.split_once(char::is_whitespace) {
            Some((atom, value)) => (atom, value),
            None => (line, ""),
        };
        Ok((atom.parse()?, T::parse(value, precedence)?))
    }
}

impl<T: AtomPolicy + Clone> Inherit for AtomPolicies<T> {
    fn inherit_from(&mut self, parent: &Self) -> anyhow::Result<()> {
        for (atom, parent_policy) in &parent.0 {
            if let Some(policy) = self.0.get_mut(atom) {
                policy.inherit_from(parent_policy)?;
            } else {
                self.0.insert(atom.clone(), parent_policy.clone());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, mem};

    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    struct TestPolicy(Vec<Box<str>>);

    impl AtomPolicy for TestPolicy {
        fn parse(value: &str, _precedence: Precedence) -> anyhow::Result<Self> {
            Ok(Self(
                value.split_ascii_whitespace().map(Into::into).collect(),
            ))
        }

        fn update_from(&mut self, other: Self) {
            self.0.extend(other.0);
        }
    }

    impl Inherit for TestPolicy {
        fn inherit_from(&mut self, parent: &Self) -> anyhow::Result<()> {
            let child = mem::take(&mut self.0);
            self.0 = parent.0.iter().cloned().chain(child).collect();
            Ok(())
        }
    }

    fn policy(values: &[&str]) -> TestPolicy {
        TestPolicy(values.iter().map(|value| (*value).into()).collect())
    }

    #[test]
    fn test_parse_atom_order() -> anyhow::Result<()> {
        let policies = AtomPolicies::<TestPolicy>::from_string(
            "
                # ignored
                dev-lang/rust first
                dev-lang/rust second # comment
                app-editors/vim third
            "
            .into(),
            Precedence::User,
        )?;

        assert_eq!(
            policies.0.get(&Atom::new("dev-lang/rust")?),
            Some(&policy(&["first", "second"]))
        );
        assert_eq!(
            policies.0.get(&Atom::new("app-editors/vim")?),
            Some(&policy(&["third"]))
        );
        assert_eq!(
            policies
                .into_iter()
                .map(|(atom, _)| atom)
                .collect::<Vec<_>>(),
            [Atom::new("dev-lang/rust")?, Atom::new("app-editors/vim")?,]
        );
        Ok(())
    }

    #[test]
    fn test_inherit_atom_order() -> anyhow::Result<()> {
        let parent = AtomPolicies::<TestPolicy>::from_string(
            "dev-lang/rust parent
                app-editors/vim inherited"
                .into(),
            Precedence::Profile(0),
        )?;
        let mut child = AtomPolicies::<TestPolicy>::from_string(
            "dev-lang/rust child
                app-editors/nano own"
                .into(),
            Precedence::Profile(1),
        )?;

        child.inherit_from(&parent)?;

        assert_eq!(
            child.0.get(&Atom::new("dev-lang/rust")?),
            Some(&policy(&["parent", "child"]))
        );
        assert_eq!(
            child.0.get(&Atom::new("app-editors/vim")?),
            Some(&policy(&["inherited"]))
        );
        assert_eq!(
            child.into_iter().map(|(atom, _)| atom).collect::<Vec<_>>(),
            [
                Atom::new("dev-lang/rust")?,
                Atom::new("app-editors/nano")?,
                Atom::new("app-editors/vim")?,
            ]
        );
        Ok(())
    }

    #[test]
    fn test_path_file_order() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let missing = temp.path().join("missing");
        let policies = AtomPolicies::<TestPolicy>::from_path(&missing, Precedence::User, true)?;
        assert!(policies.0.get(&Atom::new("dev-lang/rust")?).is_none());

        let directory = temp.path().join("package.use");
        fs::create_dir(&directory)?;
        fs::write(directory.join("b"), "dev-lang/rust second")?;
        fs::write(directory.join("a"), "dev-lang/rust first")?;

        let policies = AtomPolicies::<TestPolicy>::from_path(&directory, Precedence::User, true)?;
        assert_eq!(
            policies.0.get(&Atom::new("dev-lang/rust")?),
            Some(&policy(&["first", "second"]))
        );
        Ok(())
    }
}
