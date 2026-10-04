//! A runnable software-engineering hive and its matched single-agent baseline.
//!
//! The question this module exists to answer is whether dividing a task
//! across a desk of seats, each reading a bounded briefing instead of its own
//! growing history, beats one agent on tokens, wall clock and pass rate, with
//! the same model, tools and token meter on both sides. All I/O lives here,
//! because core is pure: the model is called through `curl`, commands run in a
//! container through `docker exec` or through the Harbor agent over stdio, and
//! telemetry goes to a JSONL file.
//!
//! | Module | Role |
//! | --- | --- |
//! | [`meter`] | the shared token meter and the run caps |
//! | [`context`] | masking / summarizing policy that bounds a seat's prompt |
//! | [`llm`] | metered chat-completions client with tool calls |
//! | [`sandbox`] | `Exec` over docker or stdio RPC, truncation, command policy |
//! | [`tools`] | tool schemas from core's speech specs plus `bash` |
//! | [`board`] | shared transcript: core commit, pins, digest, briefing |
//! | [`roles`] | prompts |
//! | [`seat`] | one activation: model loop, tools, telemetry |
//! | [`hive`] / [`single`] | the two arms |
//! | [`config`] / [`run`] | CLI and the run driver |

pub mod board;
pub mod config;
pub mod context;
pub mod hive;
pub mod llm;
pub mod meter;
pub mod roles;
pub mod run;
pub mod sandbox;
pub mod seat;
pub mod single;
pub mod tools;
