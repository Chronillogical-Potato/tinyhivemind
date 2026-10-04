//! A seat's persistent conversation and the switch that turns it off.

use serde_json::Value;
use tinyhivemind_core::runtime::Sequence;

/// Whether a seat keeps its conversation between activations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionMode {
    /// Every activation starts a new conversation from the briefing (the
    /// behaviour before persistent sessions, kept for A/B runs).
    Fresh,
    /// A seat's conversation lives for the whole run; later activations
    /// append only the desk delta, and only compaction removes messages.
    Persistent,
}

impl SessionMode {
    /// The CLI and `result.json` name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Persistent => "persistent",
        }
    }

    /// Parse a CLI value.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "fresh" => Some(Self::Fresh),
            "persistent" => Some(Self::Persistent),
            _ => None,
        }
    }
}

/// One seat's conversation and how far into the desk it has read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeatSession {
    /// The chat-completions messages, system prompt first.
    pub messages: Vec<Value>,
    /// The desk row the seat has read through; `None` before its briefing.
    pub read_through: Option<Sequence>,
    /// Activations run on this session.
    pub activations: u32,
    /// Fingerprint of the pins the seat was last shown.
    pub pins_seen: u64,
    /// The prompt size the provider last reported for this session.
    pub last_prompt: u64,
}

impl SeatSession {
    /// Whether nothing has been said in this session yet.
    #[must_use]
    pub fn is_new(&self) -> bool {
        self.messages.is_empty()
    }
}
