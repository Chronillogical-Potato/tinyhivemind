//! Translations between core's memory records and `TinyMemory`'s.
//!
//! A recalled [`ContextPack`] becomes one [`BriefingNote`] per section that
//! found something; a [`MemoryEntry`] becomes one shared learning at the
//! layout root, labelled with its kind so a failed attempt reads as one.
use tinyhivemind_core::runtime::{BriefingNote, EntryKind, MemoryEntry, Sequence};
use tinymemory_api::{LearningKind, MemoryMeta, Namespace, StoreItem};
use tinymemory_tools::ContextPack;

/// The tag every remembered entry carries, before its kind.
pub(super) const ENTRY_TAG: &str = "hive-entry";

/// How a remembered entry of `kind` is labelled in its text and tags.
pub(super) fn label(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Observation => "Observation",
        EntryKind::FailedAttempt => "Failed attempt",
        EntryKind::Outcome => "Outcome",
        EntryKind::Note => "Note",
    }
}

/// The `TinyMemory` learning kind that holds an entry of `kind`. A failed
/// attempt is a correction: it corrects the approach it tried.
fn learning_kind(kind: EntryKind) -> LearningKind {
    match kind {
        EntryKind::Observation | EntryKind::Outcome => LearningKind::Fact,
        EntryKind::FailedAttempt => LearningKind::Correction,
        EntryKind::Note => LearningKind::Other,
    }
}

/// The stable tag naming an entry's kind (`hive-entry:failed_attempt`).
fn kind_tag(kind: EntryKind) -> String {
    let name = match kind {
        EntryKind::Observation => "observation",
        EntryKind::FailedAttempt => "failed_attempt",
        EntryKind::Outcome => "outcome",
        EntryKind::Note => "note",
    };
    format!("{ENTRY_TAG}:{name}")
}

/// The fields every entry of one remember shares.
pub(super) struct EntryContext<'a> {
    /// Where shared learnings live: the layout root.
    pub at: &'a Namespace,
    /// The seat's memory agent id.
    pub agent_id: &'a str,
    /// The conversation the entries came from.
    pub conversation: &'a str,
    /// The last desk row the activation had seen.
    pub through: Option<Sequence>,
}

/// One entry as a shared learning, or `None` for a blank entry.
pub(super) fn entry_item(context: &EntryContext<'_>, entry: &MemoryEntry) -> Option<StoreItem> {
    let text = entry.text.trim();
    if text.is_empty() {
        return None;
    }
    let mut tags = vec![kind_tag(entry.kind)];
    if let Some(through) = context.through {
        tags.push(format!("desk-through:{}", through.0));
    }
    let meta = MemoryMeta {
        namespace: context.at.clone(),
        agent_id: Some(context.agent_id.to_owned()),
        thread_id: Some(context.conversation.to_owned()),
        tags,
        ..MemoryMeta::default()
    };
    Some(StoreItem::learning(
        format!("{}: {text}", label(entry.kind)),
        learning_kind(entry.kind),
        1.0,
        meta,
    ))
}

/// A pack as notes: one per section that found something, headed by the
/// section, its answer and hits as single lines, best first. Hits written by
/// `skip_agent` are left out. Lines stop once their total passes
/// `budget_chars`; the pack's sections also carry what its own budget
/// trimmed, so this is the cap that holds.
pub(super) fn notes(
    pack: &ContextPack,
    budget_chars: usize,
    skip_agent: Option<&str>,
) -> Vec<BriefingNote> {
    let mut spent = 0;
    let mut notes = Vec::new();
    'sections: for section in &pack.sections {
        let answer = section.answer.iter().map(String::as_str);
        let hits = section
            .hits
            .iter()
            .filter(|hit| skip_agent.is_none() || hit.meta.agent_id.as_deref() != skip_agent)
            .map(|hit| hit.text.as_str());
        let mut lines = Vec::new();
        for raw in answer.chain(hits) {
            let line = one_line(raw);
            if line.is_empty() || lines.contains(&line) {
                continue;
            }
            spent += line.chars().count();
            if spent > budget_chars {
                push(&mut notes, &section.heading, lines);
                break 'sections;
            }
            lines.push(line);
        }
        push(&mut notes, &section.heading, lines);
    }
    notes
}

fn push(notes: &mut Vec<BriefingNote>, heading: &str, lines: Vec<String>) {
    if !lines.is_empty() {
        notes.push(BriefingNote {
            heading: heading.to_owned(),
            lines,
        });
    }
}

/// `text` with every run of whitespace, newlines included, as one space.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
