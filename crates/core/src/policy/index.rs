use germ_pms::{Atom, CatName, PackageView, PkgName};

use crate::types::FxHashMap;

/// Exact `(category, package)` buckets, keyed by category then package.
type ExactBuckets = FxHashMap<CatName, FxHashMap<PkgName, Vec<usize>>>;
/// Category-wildcard buckets, for atoms such as `dev-lang/*`.
type CategoryBuckets = FxHashMap<CatName, Vec<usize>>;
/// Package-wildcard buckets, for atoms such as `*/rust`.
type PackageBuckets = FxHashMap<PkgName, Vec<usize>>;

/// An ordered collection of atom-keyed entries with bucketed lookup by category and package.
///
/// Entries keep their insertion order, and [`Self::visit_matches`] yields the
/// entries matching a package in that order.
pub struct AtomIndex<T> {
    entries: Box<[(Atom, T)]>,
    exact: ExactBuckets,
    by_category: CategoryBuckets,
    by_package: PackageBuckets,
    wildcard: Box<[usize]>,
}

impl<T> AtomIndex<T> {
    /// Builds an [`AtomIndex`] over `entries`, preserving their order.
    pub fn new(entries: Vec<(Atom, T)>) -> Self {
        let mut exact = ExactBuckets::default();
        let mut by_category = CategoryBuckets::default();
        let mut by_package = PackageBuckets::default();
        let mut wildcard = Vec::new();

        for (pos, (atom, _)) in entries.iter().enumerate() {
            match (atom.category(), atom.package()) {
                (Some(cat), Some(pkg)) => exact
                    .entry(cat.clone())
                    .or_default()
                    .entry(pkg.clone())
                    .or_default()
                    .push(pos),
                (Some(cat), None) => {
                    by_category.entry(cat.clone()).or_default().push(pos);
                }
                (None, Some(pkg)) => {
                    by_package.entry(pkg.clone()).or_default().push(pos);
                }
                (None, None) => wildcard.push(pos),
            }
        }

        Self {
            entries: entries.into(),
            wildcard: wildcard.into(),
            exact,
            by_category,
            by_package,
        }
    }

    /// Visits the entries matching `pkg` in insertion order.
    pub fn visit_matches<'a, P, F>(&'a self, pkg: &P, mut visit: F)
    where
        P: PackageView,
        F: FnMut(&'a T),
    {
        let empty: &[usize] = &[];
        let buckets = [
            self.exact
                .get(pkg.category())
                .and_then(|by_package| by_package.get(pkg.package()))
                .map_or(empty, Vec::as_slice),
            self.by_category
                .get(pkg.category())
                .map_or(empty, Vec::as_slice),
            self.by_package
                .get(pkg.package())
                .map_or(empty, Vec::as_slice),
            self.wildcard.as_ref(),
        ];

        // TODO: get rid of the allocation
        let mut positions = buckets.into_iter().flatten().copied().collect::<Vec<_>>();
        positions.sort_unstable();

        for pos in positions {
            let (atom, value) = &self.entries[pos];
            if pkg.matches_atom(atom) {
                visit(value);
            }
        }
    }
}
