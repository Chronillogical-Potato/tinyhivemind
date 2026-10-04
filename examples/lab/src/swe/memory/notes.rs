//! Translation between tinymemory's records and core's memory records.
//!
//! A tinymemory recall returns a [`ContextPack`] of sections of hits; core's
//! [`Recall`](tinyhivemind_core::runtime::Recall) port returns
//! [`BriefingNote`]s, which core's `frame_recalled` renders and clips. A core
//! [`RememberRequest`] becomes one stored conversation turn at the seat's
//! node, laid out exactly as `AgentMemory::post_turn` lays out its own (so
//! every tinymemory recall section finds it), but written with
//! [`WriteOptions::visible`] so a teammate's very next recall can rank it.

use tinyhivemind_core::runtime::{BriefingNote, RememberRequest};
use tinymemory_api::{
    ItemId, MemoryMeta, Namespace, Role, SourceKind, SourceRef, StoreItem, Turn, TurnRange,
};
use tinymemory_tools::ContextPack;

use super::types::kind_label;

/// Longest one recalled line is, in characters.
const LINE_CHARS: usize = 400;

/// The pack's sections as notes, best section first, and every item they
/// cite (so a rejoin can leave them out next time).
#[must_use]
pub fn pack_notes(pack: &ContextPack) -> (Vec<BriefingNote>, Vec<ItemId>) {
    let mut seen = Vec::new();
    let notes = pack
        .sections
        .iter()
        .map(|section| {
            seen.extend(section.hits.iter().map(|hit| hit.id.clone()));
            let lines = match &section.answer {
                Some(answer) if !answer.trim().is_empty() => answer
                    .lines()
                    .map(|line| line.trim().trim_start_matches("- ").trim())
                    .filter(|line| !line.is_empty())
                    .map(|line| clip(line, LINE_CHARS))
                    .collect(),
                _ => section
                    .hits
                    .iter()
                    .map(|hit| clip(&hit.text, LINE_CHARS))
                    .filter(|line| !line.is_empty())
                    .collect(),
            };
            BriefingNote {
                heading: section.heading.clone(),
                lines,
            }
        })
        .filter(|note: &BriefingNote| !note.lines.is_empty())
        .collect();
    (notes, seen)
}

/// The text of one remembered activation: one bullet per entry, its kind
/// first, so a later reader can tell a failed attempt from an observation.
#[must_use]
pub fn entries_text(request: &RememberRequest) -> String {
    request
        .entries
        .iter()
        .map(|entry| format!("- [{}] {}", kind_label(entry.kind), entry.text.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The stored item for turn `index` of `seat`'s thread at `node`.
#[must_use]
pub fn turn_item(node: Namespace, seat: &str, index: u32, text: String) -> StoreItem {
    StoreItem::Conversation {
        meta: MemoryMeta {
            namespace: node,
            thread_id: Some(seat.to_owned()),
            turns: Some(TurnRange {
                first: index,
                last: index,
            }),
            agent_id: Some(seat.to_owned()),
            source: SourceRef {
                kind: SourceKind::Conversation,
                id: Some(seat.to_owned()),
            },
            ..MemoryMeta::default()
        },
        turns: vec![Turn::new(Role::Assistant, text)],
    }
}

/// `text` on one line, at most `limit` characters.
fn clip(text: &str, limit: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= limit {
        return flat;
    }
    let head: String = flat.chars().take(limit.saturating_sub(1)).collect();
    format!("{head}…")
}
