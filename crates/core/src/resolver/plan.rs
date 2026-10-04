use super::EffectivePackage;
use crate::vdb::package::InstalledPackage;

/// Defines an execution plan for all system changes.
///
/// The plan consists of a list of operations that need
/// to be performed in order.
#[derive(Debug)]
pub struct ExecutionPlan {
    operations: Vec<PackageOperation>,
}

impl ExecutionPlan {
    /// Creates a new plan from the given `operations`.
    pub(super) const fn new(operations: Vec<PackageOperation>) -> Self {
        Self { operations }
    }

    /// Returns the list of operations in the plan.
    pub fn operations(&self) -> &[PackageOperation] {
        &self.operations
    }
}

/// Defines a single operation in the execution plan.
#[derive(Debug, Eq, PartialEq)]
pub enum PackageOperation {
    Merge(EffectivePackage),
    Replace(EffectivePackage, InstalledPackage),
    Upgrade(EffectivePackage, InstalledPackage),
    Downgrade(EffectivePackage, InstalledPackage),
    Unmerge(InstalledPackage),
}
