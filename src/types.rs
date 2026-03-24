use fxhash::FxHasher;
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasherDefault;

/// A `HashMap` using [`FxHasher`] as default.
pub type FxHashMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

/// A `HashSet` using [`FxHasher`] as default.
pub type FxHashSet<V> = HashSet<V, BuildHasherDefault<FxHasher>>;
