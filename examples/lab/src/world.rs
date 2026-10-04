//! Owned roster and desk snapshots the examples borrow from.
//!
//! Core's `Roster` and `DeskSet` are borrowed views, so every caller needs
//! somewhere to own the records. This is that somewhere.

use tinyhivemind_core::desk::{Desk, DeskSet, ResponderMode};
use tinyhivemind_core::roster::{Roster, RosterMember};

/// Agents and the desks they sit on.
#[derive(Clone, Debug, Default)]
pub struct World {
    members: Vec<RosterMember>,
    desks: Vec<Desk>,
}

impl World {
    /// An empty world.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an agent whose display name is its id with a capital.
    #[must_use]
    pub fn agent(mut self, id: &str) -> Self {
        let mut name = id.to_owned();
        if let Some(first) = name.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        self.members.push(RosterMember {
            id: id.into(),
            name: Some(name),
        });
        self
    }

    /// Declare a desk with an ordered member list.
    #[must_use]
    pub fn desk(mut self, id: &str, name: &str, purpose: &str, members: &[&str]) -> Self {
        self.desks.push(Desk {
            id: id.into(),
            name: name.into(),
            description: Some(purpose.into()),
            members: members.iter().map(|member| (*member).to_owned()).collect(),
            responder_mode: ResponderMode::Lead,
        });
        self
    }

    /// The roster view.
    #[must_use]
    pub fn roster(&self) -> Roster<'_> {
        Roster::new(&self.members, &[], &[])
    }

    /// The desk-set view.
    #[must_use]
    pub fn desks(&self) -> DeskSet<'_> {
        DeskSet::new(&self.desks, &[], &[], &[], &[])
    }

    /// The declared desks.
    #[must_use]
    pub fn desk_records(&self) -> &[Desk] {
        &self.desks
    }
}
