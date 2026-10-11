//! The result of a successful check: the recorded form of a direct edit.

use crate::{Package, patch::Patch};
use serde::{Deserialize, Serialize};

/// An accepted direct edit, classified for lineage.
///
/// `changes` holds one [`Patch`] per edit class the edit touched. Applying
/// them in order with [`crate::patch::apply`], starting from the original
/// package, passes through a valid package at every step and ends at exactly
/// `candidate`. A host records one [`crate::lineage::Record`] per change,
/// each naming the previous one as its parent, so every step keeps its own
/// class and content identity and the last identity is the candidate's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    /// One patch per touched class, in the order they apply.
    pub changes: Vec<Patch>,
    /// The edited package, decoded and validated.
    pub candidate: Package,
}
