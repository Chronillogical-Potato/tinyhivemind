//! The room's tools, served over MCP.
//!
//! `tinyhivemind::speech` states what a seat may say -- once, as data -- and
//! asks a host to render `tool_specs()` into its own tool language and map its
//! own wire onto `CallArguments`. This crate is that rendering for MCP: a
//! loopback server whose `tools/list` is the vocabulary and whose `tools/call`
//! is `interpret`, with each refusal handed back to the seat in the words
//! `UtteranceRejection` already wrote.
//!
//! It holds no episode state and runs no turn. A seat's accepted calls are
//! recorded as [`SeatEvent`]s the host drains after the turn, and its refused
//! ones as [`Refusal`]s beside them; the driver does the rest. It depends on
//! no harness: any MCP-capable one is given the same four tools.
//!
//! **Identity is structural.** A seat dials `/seat/<id>/<capability>`, the
//! capability minted when the server bound, so who is calling comes from the
//! endpoint it was handed, never from an argument. Each call also names the
//! `chat` and `parent` the host told the seat it is in, and the server checks
//! both against the turn the host [`register`](EpisodeTools::register)ed.
//!
//! This is the one socket the repository opens; ADR 0022 says why.
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use tinyhivemind_mcp::{Dispatch, EpisodeTools, serve};
//!
//! # async fn run() -> tinyhivemind_mcp::Result<()> {
//! let tools = Arc::new(EpisodeTools::new(["lead", "solver"]));
//! let server = serve(Arc::clone(&tools)).await?;
//! // Give each agent its endpoint, e.g. `McpServer::http("hive", server.endpoint("lead"))`.
//! // Before running lead's turn:
//! tools.register("lead", Dispatch { chat: "engineering".into(), parent: None });
//! // ... run the turn ...
//! tools.clear("lead");
//! for event in tools.drain("lead") {
//!     // hand `event.call` to the completion driver
//!     let _ = event;
//! }
//! for refused in tools.drain_refusals("lead") {
//!     // what the seat was told, for the host's log
//!     let _ = refused;
//! }
//! # Ok(()) }
//! ```

pub mod error;
pub mod render;
pub mod server;
pub mod tools;

pub use error::{Error, Result};
pub use render::served_specs;
pub use server::{PROTOCOL_VERSION, Server, serve};
pub use tools::{Dispatch, EpisodeTools, Refusal, SeatEvent};
