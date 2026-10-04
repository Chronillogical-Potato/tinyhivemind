//! The hive's memory tools, served over a host's [`WorkingMemory`].
//!
//! Three tools -- `hive_memory_recall`, `hive_memory_note`, `hive_memory_forget` -- let a seat
//! carry observations, claims and dead ends across activations. They are the
//! hive's half of the contract: names, descriptions, argument schemas and the
//! words a seat is answered in. The other half is the host's: the
//! [`WorkingMemory`] it hands to [`MemoryTools::new`], which may be a markdown
//! file, a vector store or a memory service. This module never learns which.
//!
//! The seat is named by the host, never by the call: [`MemoryTools::call`]
//! takes the seat id the host is running, so a seat cannot write a note as
//! someone else or recall another seat's private entries.
//!
//! # Example
//!
//! ```
//! use tinyhivemind_tools::memory_tool_definitions;
//!
//! let names: Vec<_> = memory_tool_definitions().into_iter().map(|t| t.name).collect();
//! assert_eq!(names, ["hive_memory_recall", "hive_memory_note", "hive_memory_forget"]);
//! ```

use serde_json::{Value, json};
use std::sync::Arc;
use tinyhivemind_core::runtime::{
    Error, MEMORY_LIMIT, MEMORY_NOTE_CHARS, MemoryNote, MemoryScope, WorkingMemory, memory,
};
use tinytools::ToolSpec;

/// The tool names this module serves, in the order a seat meets them.
pub const MEMORY_TOOLS: [&str; 3] = [
    "hive_memory_recall",
    "hive_memory_note",
    "hive_memory_forget",
];

/// The memory tools as native `tinytools::ToolSpec` values.
#[must_use]
pub fn memory_tool_definitions() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "hive_memory_recall".to_owned(),
            description: "Look up what the hive already knows before repeating work: findings, \
                          failed attempts, decisions. Check here first when you resume."
                .to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "What you want to remember. Empty returns the most relevant entries."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MEMORY_LIMIT,
                        "default": 5
                    }
                },
                "required": []
            }),
        },
        ToolSpec {
            name: "hive_memory_note".to_owned(),
            description: format!(
                "Remember something for later: a finding, a command that failed and why, a \
                 decision. One fact per note, at most {MEMORY_NOTE_CHARS} characters."
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "The fact to remember." },
                    "scope": {
                        "type": "string",
                        "enum": ["hive", "seat"],
                        "default": "hive",
                        "description": "`hive` is shared with every seat; `seat` is private to you."
                    }
                },
                "required": ["text"]
            }),
        },
        ToolSpec {
            name: "hive_memory_forget".to_owned(),
            description:
                "Drop an entry that is wrong or stale, by the id hive_memory_recall showed."
                    .to_owned(),
            parameters: json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }),
        },
    ]
}

/// The memory tools over one host-supplied engine.
#[derive(Clone)]
pub struct MemoryTools {
    memory: Arc<dyn WorkingMemory>,
}

impl std::fmt::Debug for MemoryTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryTools").finish_non_exhaustive()
    }
}

impl MemoryTools {
    /// Serve the memory tools over `memory`.
    #[must_use]
    pub fn new(memory: Arc<dyn WorkingMemory>) -> Self {
        Self { memory }
    }

    /// Whether `name` is one of the memory tools.
    #[must_use]
    pub fn serves(name: &str) -> bool {
        MEMORY_TOOLS.contains(&name)
    }

    /// Run one memory tool call for `seat`.
    ///
    /// # Errors
    ///
    /// Returns the text to hand the seat back when the tool is unknown, an
    /// argument is missing or malformed, or the host's engine refuses or
    /// fails. The error never carries the engine's own message.
    pub async fn call(&self, seat: &str, name: &str, arguments: &Value) -> Result<String, String> {
        match name {
            "hive_memory_recall" => {
                let query = arguments.get("query").and_then(Value::as_str).unwrap_or("");
                let limit = arguments
                    .get("limit")
                    .and_then(Value::as_u64)
                    .map_or(5, |n| usize::try_from(n).unwrap_or(MEMORY_LIMIT));
                let found = memory::recall(self.memory.as_ref(), seat, query, limit)
                    .await
                    .map_err(|_| "memory is unavailable; carry on without it".to_owned())?;
                if found.is_empty() {
                    return Ok("nothing remembered".to_owned());
                }
                Ok(found
                    .iter()
                    .map(|entry| format!("{} [{}] {}", entry.id, entry.author, entry.text))
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            "hive_memory_note" => {
                let text = arguments
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or("hive_memory_note needs `text`")?;
                let scope = match arguments.get("scope").and_then(Value::as_str) {
                    None | Some("hive") => MemoryScope::Hive,
                    Some("seat") => MemoryScope::Seat,
                    Some(other) => {
                        return Err(format!("unknown scope `{other}`: use hive or seat"));
                    }
                };
                let note = MemoryNote {
                    author: seat.to_owned(),
                    scope,
                    text: text.to_owned(),
                };
                match memory::record(self.memory.as_ref(), &note).await {
                    Ok(entry) => Ok(format!("remembered as {}", entry.id)),
                    Err(error @ (Error::MemoryNoteEmpty | Error::MemoryNoteTooLong { .. })) => {
                        Err(error.to_string())
                    }
                    Err(_) => Err("memory is unavailable; carry on without it".to_owned()),
                }
            }
            "hive_memory_forget" => {
                let id = arguments
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("hive_memory_forget needs `id`")?;
                self.memory
                    .forget(seat, id)
                    .await
                    .map(|()| format!("forgot {id}"))
                    .map_err(|_| "memory is unavailable; carry on without it".to_owned())
            }
            other => Err(format!("unknown memory tool `{other}`")),
        }
    }
}

#[cfg(test)]
mod test;
