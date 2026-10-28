mod keywords;
mod useflags;

use std::path::Path;

use germ_pms::Atom;
pub(crate) use keywords::{KeywordRule, PackageAcceptKeywords};
pub(crate) use useflags::{MatchedUse, PackageUseRecords, UseFlags};

use crate::files::entry::{LineOrigin, Precedence};
use crate::files::parse_from_path;
#[cfg(test)]
use crate::files::test_support::with_origins;
use crate::types::FxIndexMap;
use crate::utils::Inherit;

/// This trait abstracts multiple values in a line-based file, such as `package.use`.
pub trait AtomPolicy: Default + Inherit {
    fn parse(value: &str, precedence: Precedence, origin: LineOrigin) -> anyhow::Result<Self>;
    fn update_from(&mut self, other: Self);
}

/// Maps atoms to their policies while preserving insertion order.
#[derive(Clone, Debug, Default)]
pub struct AtomPolicies<T: AtomPolicy>(FxIndexMap<Atom, T>);

impl<T: AtomPolicy> AtomPolicies<T> {
    pub fn from_path(path: &Path, precedence: Precedence, recursive: bool) -> anyhow::Result<Self> {
        let pairs = parse_from_path(path, recursive, true, |text, origin| {
            Self::parse_line(text, precedence, origin)
        })?;
        Ok(Self::from_pairs(pairs))
    }

    fn parse_line(
        line: &str,
        precedence: Precedence,
        origin: LineOrigin,
    ) -> anyhow::Result<(Atom, T)> {
        let (atom, value) = match line.split_once(char::is_whitespace) {
            Some((atom, value)) => (atom, value),
            None => (line, ""),
        };
        Ok((atom.parse()?, T::parse(value, precedence, origin)?))
    }

    /// Merges `pairs` into one policy per atom, preserving order.
    fn from_pairs(pairs: impl IntoIterator<Item = (Atom, T)>) -> Self {
        let mut policies = FxIndexMap::<Atom, T>::default();
        for (atom, policy) in pairs {
            policies.entry(atom).or_default().update_from(policy);
        }
        Self(policies)
    }

    #[cfg(test)]
    pub fn from_lines<'a>(
        lines: impl IntoIterator<Item = &'a str>,
        precedence: Precedence,
    ) -> anyhow::Result<Self> {
        let pairs = with_origins(lines)
            .map(|(line, origin)| Self::parse_line(line, precedence, origin))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self::from_pairs(pairs))
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

impl<T: AtomPolicy> IntoIterator for AtomPolicies<T> {
    type Item = (Atom, T);
    type IntoIter = <FxIndexMap<Atom, T> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use std::mem;

    use super::*;

    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    struct TestPolicy(Vec<Box<str>>);

    impl AtomPolicy for TestPolicy {
        fn parse(
            value: &str,
            _precedence: Precedence,
            _origin: LineOrigin,
        ) -> anyhow::Result<Self> {
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
    fn test_atom_policies() -> anyhow::Result<()> {
        let policies = AtomPolicies::<TestPolicy>::from_lines(
            [
                "dev-lang/rust first",
                "dev-lang/rust second",
                "app-editors/vim third",
            ],
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

        let parent = AtomPolicies::<TestPolicy>::from_lines(
            ["dev-lang/rust parent", "app-editors/vim inherited"],
            Precedence::Profile(0),
        )?;
        let mut child = AtomPolicies::<TestPolicy>::from_lines(
            ["dev-lang/rust child", "app-editors/nano own"],
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
}
