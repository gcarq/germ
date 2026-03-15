use fxhash::FxHasher64;
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasherDefault;

/// A `HashMap` using a default Fx hasher.
pub type FxHashMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher64>>;

/// A `HashSet` using a default Fx hasher.
pub type FxHashSet<V> = HashSet<V, BuildHasherDefault<FxHasher64>>;
