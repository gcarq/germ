use crate::types::FxHashSet;
use std::hash;

/// Holds the result of entering a node during traversal.
pub enum Visit {
    Continue,       // Should be visited
    AlreadyVisited, // Can be skipped
}

/// A cycle detected during traversal.
///
/// `path` lists the nodes of the active recursion path.
pub struct Cycle<K> {
    pub path: Vec<K>,
}

/// Tracks DFS traversal state and detects cycles.
///
/// Entering a node that is already on the path is reported as [`Cycle`].
/// Visited nodes are reported as [`Visit::AlreadyVisited`] on later visits.
pub struct DfsState<K> {
    visiting: FxHashSet<K>,
    visited: Option<FxHashSet<K>>,
    stack: Vec<K>,
}

impl<K> DfsState<K>
where
    K: Clone + Eq + hash::Hash,
{
    /// Starts visiting `key` or reports it as [`Cycle`].
    pub fn enter(&mut self, key: &K) -> Result<Visit, Cycle<K>> {
        if self.visited.as_ref().is_some_and(|v| v.contains(key)) {
            return Ok(Visit::AlreadyVisited);
        }

        if !self.visiting.insert(key.clone()) {
            let start = self.stack.iter().position(|n| n == key).unwrap_or_default();
            let mut path = self.stack[start..].to_vec();
            path.push(key.clone());
            return Err(Cycle { path });
        }

        self.stack.push(key.clone());
        Ok(Visit::Continue)
    }

    /// Marks `key` as processed.
    pub fn leave(&mut self, key: &K) {
        self.visiting.remove(key);
        self.stack.pop();
        if let Some(visited) = &mut self.visited {
            visited.insert(key.clone());
        }
    }
}

impl<K> Default for DfsState<K> {
    fn default() -> Self {
        Self {
            visiting: FxHashSet::default(),
            visited: Some(FxHashSet::default()),
            stack: Vec::default(),
        }
    }
}
