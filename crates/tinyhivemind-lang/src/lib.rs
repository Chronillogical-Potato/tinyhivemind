//! The hive language: a declarative, portable definition of a hive of agents,
//! validated and lowered into core values without opening storage.
//!
//! A *package* is a map from relative path to file contents. Three canonical
//! JSON files carry the typed definition, and Markdown side documents carry
//! the prose:
//!
//! | Path | Decoded into | Holds |
//! | --- | --- | --- |
//! | `hive.json` | [`Manifest`] | roles, seats, desks, goals, missions, topology, referrals, context paths, core policies, coordinator settings |
//! | `constitution.json` | [`Constitution`] | host-pinned caps, permitted edit classes, protected namespaces, canaries, opaque evaluator settings |
//! | `memory.json` | [`MemorySpec`] | memory identities and seat bindings, never remembered contents |
//! | `roles/*.md` | [`Package::documents`] | one charter per role |
//! | `seats/*.md` | [`Package::documents`] | one prompt per seat, with optional JSON frontmatter |
//! | `context/*.md` | [`Package::documents`] | shared documents split into stable `<!-- item:id -->` items |
//!
//! # Entry points
//!
//! - [`parse`] decodes a caller-supplied document map into a [`Package`] and
//!   validates it.
//! - [`validate`] checks references, paths, policy bounds against the
//!   constitution, frontmatter agreement, and memory reaches.
//!   [`validate_path`] and [`validate_namespace`] expose its path and
//!   namespace checks on their own.
//! - [`lower`] turns a valid package into a [`LoweredHive`]: core desks,
//!   routing candidates, and per-seat factory inputs the host activates.
//! - [`patch::apply`], [`patch::guard`] and [`patch::diff`] apply, check and
//!   compute a classified editor proposal against an immutable constitution.
//! - [`lineage::content_id`], [`lineage::record`], [`lineage::accepted_head`]
//!   and [`lineage::pareto_frontier`] address evaluated designs by content and
//!   fold an archive of them.
//!
//! # What this crate does not hold
//!
//! It reads no files: the host collects the document map and hands it to
//! [`parse`]. It runs no agents and calls no model: [`lower`] produces the
//! inputs a host needs to create seats, and the host decides when to activate
//! them. It owns no evaluation, archive storage, or memory store: those are
//! host-owned, and [`lineage`] only folds records the host supplies. It does
//! not convert to `OpenHuman` types either; that adapter lives in the
//! `tinyhivemind-openhuman` crate's `language` module, so this crate stays
//! host-neutral.
//!
//! # Example
//!
//! ```
//! use std::collections::BTreeMap;
//! use tinyhivemind_lang::{CoordinatorSettings, Policies, lower, parse};
//!
//! // Every core policy is spelled out on the wire; this example takes the
//! // defaults rather than repeating them.
//! let hive = serde_json::json!({
//!     "$schema": "https://tinyhumans.ai/schemas/hive/v1",
//!     "version": 1,
//!     "id": "review",
//!     "name": "Review board",
//!     "roles": [{ "id": "reviewer", "charter": "roles/reviewer.md" }],
//!     "seats": [{
//!         "id": "reviewer",
//!         "label": "Reviewer",
//!         "role": "reviewer",
//!         "prompt": "seats/reviewer.md",
//!         "template": "default",
//!         "config": {},
//!         "model": null,
//!         "tools": { "allow": ["file_read"], "deny": [] },
//!         "mcp": [],
//!         "skills": []
//!     }],
//!     "desks": [{
//!         "id": "board",
//!         "name": "Board",
//!         "description": "review one change",
//!         "members": ["reviewer"],
//!         "responder_mode": "auto"
//!     }],
//!     "goals": [],
//!     "missions": [],
//!     "topology": [],
//!     "referrals": [],
//!     "context": [],
//!     "policies": Policies::default(),
//!     "coordinator": CoordinatorSettings::default(),
//! });
//! let constitution = serde_json::json!({
//!     "hard_constraints": [],
//!     "max_turns": 60,
//!     "max_width": 8,
//!     "permitted_edits": [],
//!     "read_only_memories": [],
//!     "protected_namespaces": [],
//!     "contamination_canaries": [],
//!     "evaluation": null,
//!     "acceptance": null,
//!     "telemetry": null
//! });
//! let memory = serde_json::json!({ "root": "team:hive", "identities": [], "bindings": [] });
//!
//! let documents = BTreeMap::from([
//!     ("hive.json".to_owned(), hive.to_string()),
//!     ("constitution.json".to_owned(), constitution.to_string()),
//!     ("memory.json".to_owned(), memory.to_string()),
//!     ("roles/reviewer.md".to_owned(), "Judge changes on evidence.\n".to_owned()),
//!     (
//!         "seats/reviewer.md".to_owned(),
//!         "---\n{\"id\":\"reviewer\",\"role\":\"reviewer\"}\n---\nCite a line for every finding.\n"
//!             .to_owned(),
//!     ),
//! ]);
//!
//! let package = parse(&documents)?;
//! let hive = lower(&package)?;
//! let seats: Vec<_> = hive.seats.iter().map(|seat| seat.id.as_str()).collect();
//! assert_eq!(seats, ["reviewer"]);
//! // Lowering hands the host the prompt body without its frontmatter.
//! assert_eq!(hive.seats[0].prompt, "Cite a line for every finding.\n");
//! assert_eq!(hive.desks[0].members, ["reviewer"]);
//! # Ok::<(), tinyhivemind_lang::Error>(())
//! ```
pub mod document;
pub mod error;
pub mod lineage;
pub mod lowering;
pub mod memory;
pub mod patch;
pub mod validation;
pub use document::*;
pub use error::{Error, Result};
pub use lowering::{LoweredHive, LoweredMemoryBinding, LoweredSeat, lower};
pub use memory::*;
pub use validation::{validate, validate_namespace, validate_path};
