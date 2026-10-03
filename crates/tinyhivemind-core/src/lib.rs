//! Host-neutral coordination for agents sharing a transcript.
//!
//! The top-level algebra resolves desks, rosters, mentions, approval, and
//! responder decisions from data the host supplies. [`runtime`] projects a
//! host-owned log and exposes waiting ports. [`hive`] folds task division and
//! bounded deliberation rounds. [`embed`] handles conversation routing,
//! [`typesafe`] builds exact System One questions through a transport port,
//! and [`driver`] coordinates completion episodes over host-bound handles.
//!
//! # No owned IO
//!
//! This crate opens no database, file, or socket and owns no agent session.
//! The host supplies snapshots, a session log, a model transport, and durable
//! commits. Pure decisions stay separate from those waiting boundaries inside
//! focused modules. The crate links no harness or async runtime; the
//! dependency check in `.github/scripts/assert-pure.sh` guards that boundary.
//!
//! # Layout
//!
//! Each feature area lives in its own module directory with a `mod.rs` module
//! root, an optional `types.rs`, and a `test.rs` holding its unit tests. The
//! public surface is namespaced by module rather than flattened here: this
//! crate holds distinct desk, roster, mention, and policy concerns.
//!
//! # Modules
//!
//! - [`chat`] — conversation identity: which stored chat id names which
//!   conversation, and the four spellings that mean the default desk.
//! - [`approval`] — total, fail-closed authorization for one typed action.
//! - [`desk`] — host-compatible desk records and the borrowed overlay fold.
//! - [`dispatch`] — bounded selection of at most one mentioned child turn.
//! - [`error`] — typed failures from malformed records or unresolved desks.
//! - [`masking`] — the one code scanner every authored grammar shares: which
//!   spans of a body are fenced or inline code, and so carry no grammar.
//! - [`mention`] — authored mention parsing and pure routing choices.
//! - [`referral`] — bounded selection of one child turn that may cross a desk,
//!   and the one answer that comes back.
//! - [`roster`] — borrowed agent and person identity snapshots, and the
//!   three states an agent can be in: active, retired, or tombstoned.
//! - [`responder`] — deterministic selection of one agent for one message.
//!
//! - [`runtime`] — session ports, projection, sharing, pins, and digest.
//! - [`hive`] — traces, salience, task division, completion, and quorum.
//! - [`embed`] — host-neutral conversation surfaces and semantic routing.
//! - [`typesafe`] — System One wire types and `JevRouter`.
//! - [`driver`] — bound desks, completion scheduling, and conducted episodes.
//!
//! # Example
//!
//! ```
//! use tinyhivemind_core::chat::{is_general_chat, same_conversation};
//!
//! // All four stored spellings of the default desk are one conversation.
//! assert!(is_general_chat(None));
//! assert!(same_conversation(Some("main"), Some("General")));
//!
//! // Everything else compares verbatim.
//! assert!(!same_conversation(Some("engineering"), Some("Engineering")));
//!
//! use tinyhivemind_core::desk::{Desk, DeskSet, ResponderMode};
//!
//! let desks = [Desk {
//!     id: "engineering".into(),
//!     name: "Engineering".into(),
//!     description: Some("Build the product".into()),
//!     members: vec!["alice".into(), "bob".into()],
//!     responder_mode: ResponderMode::Lead,
//! }];
//! let set = DeskSet::new(&desks, &[], &[], &[], &[]);
//! assert_eq!(set.lead("Engineering")?, Some("alice"));
//!
//! use tinyhivemind_core::{
//!     mention::{MentionAuthor, MentionTarget, direct_responder, resolve},
//!     roster::{Roster, RosterMember},
//! };
//! let members = [RosterMember {
//!     id: "alice".into(),
//!     name: Some("Alice".into()),
//! }];
//! let roster = Roster::new(&members, &[], &[]);
//! let mentions = resolve(
//!     "Could you take this, @Alice?",
//!     None,
//!     &MentionAuthor::Other,
//!     &roster,
//!     &set,
//! );
//! assert_eq!(direct_responder(&mentions, &roster), Some("alice"));
//! assert_eq!(mentions[0].target, MentionTarget::Agent { id: "alice".into() });
//! # Ok::<(), tinyhivemind_core::error::Error>(())
//! ```

pub mod approval;
pub mod aside;
pub mod chat;
pub mod desk;
pub mod dispatch;
pub mod error;
pub mod masking;
pub mod mention;
pub mod referral;
pub mod responder;
pub mod roster;

/// Host-neutral completion driver.
pub mod driver;
/// Host-neutral conversation and semantic routing.
pub mod embed;
/// Bounded group deliberation and completion folds.
pub mod hive;
/// Runtime-neutral session projection and ports.
pub mod runtime;
/// TypeSafe System One wire and Jev router.
pub mod typesafe;
