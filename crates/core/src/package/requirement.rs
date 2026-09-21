use super::PackageView;
use crate::atom::Atom;
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
            if !dep.is_satisfied_by(owner_use, target_use)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::pkg;
    use crate::useflag::test_support::effective;

    #[test]
    fn test_root_ignores_use_deps() {
        let pkg = pkg("app-misc", "foo", "1.0", &[("IUSE", "")]);
        let atom = "app-misc/foo[foo]".parse().unwrap();
        let request = AtomRequirement::root(&atom);

        let result = request
            .satisfied_by(&pkg, &effective(&["foo"], &[]))
            .unwrap();
        assert!(result);
    }

    #[test]
    fn test_static_mismatch() {
        let pkg = pkg("app-misc", "foo", "1.0", &[("IUSE", "")]);
        let atom = "app-misc/bar".parse().unwrap();

        let request = AtomRequirement::root(&atom);
        assert!(!request.satisfied_by(&pkg, &effective(&[], &[])).unwrap());
    }
}
