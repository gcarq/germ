use fxhash::FxHasher;
use indexmap::{IndexMap, IndexSet};
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasherDefault;

/// A [`HashMap`] using [`FxHasher`] as default.
pub type FxHashMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

/// An [`IndexMap`] using [`FxHasher`] as default.
pub type FxIndexMap<K, V> = IndexMap<K, V, BuildHasherDefault<FxHasher>>;

/// A [`HashSet`] using [`FxHasher`] as default.
pub type FxHashSet<V> = HashSet<V, BuildHasherDefault<FxHasher>>;

/// An [`IndexSet`] using [`FxHasher`] as default.
pub type FxIndexSet<V> = IndexSet<V, BuildHasherDefault<FxHasher>>;
