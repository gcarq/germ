use germ_pms::{Atom, UseDep, UseDepDefault, UseDepKind};

use super::PackageView;
use crate::useflag::EffectiveUse;

/// Represents an [`Atom`] with the effective USE state of the package expressing it.
///
/// An atom USE dependency describes how a targets USE flag relates to the package
/// expressing the atom, so it is checked against both effective USE states.
#[derive(Clone, Copy, Debug)]
pub enum AtomRequirement<'a> {
    /// A root requirement whose atom USE deps are ignored,
    /// this usually represents a package provided by the user.
    Root(&'a Atom),
    /// A dependency requirement expressed by a package.
    Dependency {
        atom: &'a Atom,
        owner_use: &'a EffectiveUse,
    },
}

impl<'a> AtomRequirement<'a> {
    /// Creates a new root requirement.
    pub const fn root(atom: &'a Atom) -> Self {
        Self::Root(atom)
    }

    /// Creates a dependency requirement.
    pub const fn dependency(atom: &'a Atom, owner_use: &'a EffectiveUse) -> Self {
        Self::Dependency { atom, owner_use }
    }

    /// Returns the underlying [`Atom`].
    pub const fn atom(&self) -> &'a Atom {
        match self {
            Self::Root(atom) => atom,
            Self::Dependency { atom, .. } => atom,
        }
    }

    /// Returns whether the given `package` satisfies this requirement.
    pub fn satisfied_by<P>(&self, pkg: &P, target_use: &EffectiveUse) -> anyhow::Result<bool>
    where
        P: PackageView,
    {
        let atom = self.atom();
        if !pkg.matches_atom(atom) {
            return Ok(false);
        }

        let Self::Dependency { atom, owner_use } = &self else {
            return Ok(true);
        };

        for dep in atom.use_deps() {
            if !use_dep_satisfied_by(dep, owner_use, target_use)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Returns whether this dependency is satisfied by the `owner` and `target` package USE states.
///
/// TODO: This currently dosn't check whether a USE flag is available for `target`.
fn use_dep_satisfied_by(
    dep: &UseDep,
    owner: &EffectiveUse,
    target: &EffectiveUse,
) -> anyhow::Result<bool> {
    let target = target
        .state(dep.flag())
        .or_else(|| dep.default().map(UseDepDefault::is_enabled));

    let matches = match dep.kind() {
        UseDepKind::Enabled => target == Some(true),
        UseDepKind::Disabled => target == Some(false),
        UseDepKind::ConditionalEnabled => target == Some(true) || !owner.is_enabled(dep.flag())?,
        UseDepKind::ConditionalDisabled => target == Some(false) || owner.is_enabled(dep.flag())?,
        UseDepKind::Equal => target == Some(owner.state(dep.flag()).unwrap_or(false)),
        UseDepKind::NotEqual => target == Some(!owner.state(dep.flag()).unwrap_or(false)),
    };
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use germ_pms::UseDep;

    use super::*;
    use crate::test_support::pkg;
    use crate::useflag::test_support::effective;

    #[test]
    fn test_root_ignores_use_deps() {
        let pkg = pkg("app-misc", "foo", "1.0", &[("IUSE", "")]);
        let atom = "app-misc/foo[foo]".parse().unwrap();
        let request = AtomRequirement::root(&atom);

        let result = request.satisfied_by(&pkg, &effective(&["foo"], &[]));
        assert!(result.unwrap());
    }

    #[test]
    fn test_use_dep_matching() {
        type EffectiveArgs<'a> = (&'a [&'a str], &'a [&'a str]);

        let cases: [(&str, EffectiveArgs<'_>, EffectiveArgs<'_>, bool); 24] = [
            ("foo", (&["foo"], &["foo"]), (&["foo"], &["foo"]), true),
            ("foo", (&["foo"], &["foo"]), (&["foo"], &[]), false),
            ("foo", (&["foo"], &["foo"]), (&[], &[]), false),
            ("-foo", (&["foo"], &["foo"]), (&["foo"], &[]), true),
            ("-foo", (&["foo"], &["foo"]), (&["foo"], &["foo"]), false),
            ("-foo", (&["foo"], &["foo"]), (&[], &[]), false),
            ("foo?", (&["foo"], &["foo"]), (&["foo"], &["foo"]), true),
            ("foo?", (&["foo"], &["foo"]), (&["foo"], &[]), false),
            ("foo?", (&["foo"], &[]), (&[], &[]), true),
            ("!foo?", (&["foo"], &[]), (&["foo"], &[]), true),
            ("!foo?", (&["foo"], &[]), (&["foo"], &["foo"]), false),
            ("!foo?", (&["foo"], &["foo"]), (&[], &[]), true),
            ("foo=", (&["foo"], &["foo"]), (&["foo"], &["foo"]), true),
            ("foo=", (&["foo"], &[]), (&["foo"], &[]), true),
            ("foo=", (&[], &[]), (&["foo"], &[]), true),
            ("foo=", (&[], &[]), (&["foo"], &["foo"]), false),
            ("!foo=", (&["foo"], &["foo"]), (&["foo"], &[]), true),
            ("!foo=", (&["foo"], &[]), (&["foo"], &["foo"]), true),
            ("!foo=", (&[], &[]), (&["foo"], &["foo"]), true),
            ("!foo=", (&[], &[]), (&["foo"], &[]), false),
            ("foo(+)", (&[], &[]), (&[], &[]), true),
            ("foo(-)", (&[], &[]), (&[], &[]), false),
            ("foo(+)=", (&["foo"], &[]), (&[], &[]), false),
            ("foo(-)=", (&["foo"], &[]), (&[], &[]), true),
        ];

        for (input, owner, target, expected) in cases {
            let dep = input.parse::<UseDep>().unwrap();
            let owner = effective(owner.0, owner.1);
            let target = effective(target.0, target.1);
            let actual = use_dep_satisfied_by(&dep, &owner, &target).unwrap();
            assert_eq!(actual, expected, "{input}");
        }
    }

    #[test]
    fn test_use_dep_conditional_owner() {
        let euse = EffectiveUse::default();
        for input in ["foo?", "!foo?"] {
            let dep = input.parse::<UseDep>().unwrap();
            let result = use_dep_satisfied_by(&dep, &euse, &euse);
            assert!(result.is_err(), "{input}");
        }
    }

    #[test]
    fn test_static_mismatch() {
        let pkg = pkg("app-misc", "foo", "1.0", &[("IUSE", "")]);
        let atom = "app-misc/bar".parse().unwrap();

        let result = AtomRequirement::root(&atom).satisfied_by(&pkg, &EffectiveUse::default());
        assert!(!result.unwrap());
    }
}
