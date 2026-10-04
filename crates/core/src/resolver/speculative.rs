use super::Resolver;
use super::provider::PackageLookup;
use super::state::StateCheckpoint;

/// A checkpoint for one speculative resolver traversal.
pub struct SpeculativeTraversal {
    checkpoint: StateCheckpoint,
}

impl SpeculativeTraversal {
    /// Rolls back state changes made during the traversal.
    pub fn rollback<U: PackageLookup>(self, resolver: &mut Resolver<U>) {
        resolver.state.rollback(self.checkpoint);
    }
}

impl<U: PackageLookup> Resolver<U> {
    /// Starts a speculative traversal at the current resolver state.
    pub fn start_traversal(&self) -> SpeculativeTraversal {
        SpeculativeTraversal {
            checkpoint: self.state.checkpoint(),
        }
    }
}
