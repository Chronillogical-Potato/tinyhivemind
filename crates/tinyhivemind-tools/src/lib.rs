//! The episode's tools, as a record a host drains.
//!
//! `tinyhivemind_core::runtime::speech` states what a seat may say -- once, as data -- and
//! asks a host to render `tool_specs()` into its own tool language and map its
//! own call arguments onto `CallArguments`. This crate provides the served
//! specs as native `tinytools::ToolSpec` values
//! ([`tool_definitions`]), and [`EpisodeTools`], which holds per seat the turn
//! the host opened, the rows it may `read`, and the calls it made -- and
//! decides each call in [`EpisodeTools::call`]: check the turn, check the
//! thread, `interpret`, record the event or the refusal.
//!
//! A harness wraps each definition in its native tool type and calls `call`
//! from its execute method. The host drains the
//! [`SeatEvent`]s and [`Refusal`]s after the turn.
//!
//! It holds no episode state beyond the open turn and runs no turn: the
//! driver does the rest. It opens no socket and awaits nothing.
//!
//! # Example
//!
//! ```
//! use tinyhivemind_tools::{Dispatch, EpisodeTools};
//!
//! let tools = EpisodeTools::new(["lead", "solver"]);
//! // Before running lead's turn: which chat and thread its calls must name.
//! tools.register("lead", Dispatch { chat: "engineering".into(), parent: None });
//! // A native tool's execute:
//! let receipt = tools.call(
//!     "lead",
//!     "complete_episode",
//!     &serde_json::json!({"message": "done", "chat": "engineering", "parent": null}),
//! );
//! assert_eq!(receipt.as_deref(), Ok("recorded: your assignment is complete"));
//! tools.clear("lead");
//! assert_eq!(tools.drain("lead").len(), 1);
//! ```

pub mod memory;
pub mod render;
pub mod tools;

pub use memory::{MEMORY_TOOLS, MemoryTools, memory_tool_definitions};
pub use render::{raw_arguments, served_specs, tool_definitions};
pub use tools::{Dispatch, EpisodeTools, Refusal, SeatEvent};
