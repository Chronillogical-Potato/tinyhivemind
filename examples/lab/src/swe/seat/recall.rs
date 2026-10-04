//! Where an activation meets its session and core's memory ports.
//!
//! [`open`] starts or resumes the session (a resume is a typed
//! `session_resumed` event); [`ask`] is a core `Recall` at one
//! `RecallMoment`, framed by core's `frame_recalled` and reported as a typed
//! `recalled` event; [`remember`] is a core `Remember` of the activation's
//! ledger and last words, reported as a typed `remembered` event. A failed
//! call also leaves a `memory` mark with the error, since the typed events
//! carry no reason. [`entry`] turns one executed command into a ledger line.

use std::time::Instant;

use serde_json::json;
use tinyhivemind_core::runtime::{
    EntryKind, MemoryEntry, RecallMoment, RecallRequest, RememberRequest, frame_recalled,
};
use tinyhivemind_core::telemetry::TraceEvent;

use super::super::memory::LedgerEntry;
use super::super::sandbox::truncate;
use super::super::session::SeatSession;
use super::{Activation, Env, Outcome, Work};
use crate::block_on;

/// Longest command kept in a ledger line, in bytes.
const LEDGER_CMD: usize = 200;
/// Longest outcome kept in a ledger line, in characters.
const LEDGER_OUTCOME: usize = 160;

/// Start `session` (system prompt, then the opening with a session-start
/// block in front) or resume it (the delta, with a rejoin block after).
pub(super) fn open(env: &Env<'_>, act: &Activation<'_>, session: &mut SeatSession) {
    if session.is_new() {
        let user = match ask(env, act, RecallMoment::SessionStart) {
            Some(block) => format!("{block}\n\n{}", act.user),
            None => act.user.clone(),
        };
        session
            .messages
            .push(json!({ "role": "system", "content": act.system }));
        session
            .messages
            .push(json!({ "role": "user", "content": user }));
        return;
    }
    env.tracer.emit(TraceEvent::SessionResumed {
        seat: act.seat.to_owned(),
        messages: count(session.messages.len()),
        delta_rows: count(act.shown_rows),
    });
    let user = match ask(env, act, RecallMoment::Rejoin) {
        Some(block) => format!("{}\n\n{block}", act.user),
        None => act.user.clone(),
    };
    session
        .messages
        .push(json!({ "role": "user", "content": user }));
}

/// Recall for the activation's seat at `moment` and frame the notes, or
/// `None` without memory or when nothing came back.
pub(super) fn ask(env: &Env<'_>, act: &Activation<'_>, moment: RecallMoment) -> Option<String> {
    let memory = env.memory?;
    let budget = memory.budget_chars();
    let request = RecallRequest {
        seat: act.seat.to_owned(),
        conversation: memory.conversation(),
        focus: Some(act.focus.clone()).filter(|f| !f.trim().is_empty()),
        moment,
        budget_chars: budget,
    };
    let started = Instant::now();
    let outcome = block_on(memory.recall(&request));
    let latency_ms = elapsed_ms(started);
    let (notes, framed) = match outcome {
        Ok(notes) => {
            let framed = frame_recalled(&notes, budget);
            (notes.len(), framed)
        }
        Err(error) => {
            failed(env, act.seat, "recall", request.moment.label(), &error);
            (0, None)
        }
    };
    env.tracer.emit(TraceEvent::Recalled {
        seat: act.seat.to_owned(),
        moment: request.moment.label().to_owned(),
        notes: count(notes),
        chars: framed.as_ref().map_or(0, |b| b.chars().count() as u64),
        latency_ms,
    });
    framed
}

/// Hand memory the commands not yet stored and the seat's latest words.
pub(super) fn remember(
    env: &Env<'_>,
    act: &Activation<'_>,
    session: &SeatSession,
    out: &Outcome,
    work: &mut Work,
) {
    let Some(memory) = env.memory else { return };
    let mut entries: Vec<MemoryEntry> = out.ledger[work.remembered.min(out.ledger.len())..]
        .iter()
        .map(LedgerEntry::to_entry)
        .collect();
    work.remembered = out.ledger.len();
    let (kind, text) = match &out.spoke {
        Some(done) => (EntryKind::Outcome, done.content.clone()),
        None => (EntryKind::Note, work.last_text.clone()),
    };
    if !text.trim().is_empty() {
        entries.push(MemoryEntry { kind, text });
    }
    if entries.is_empty() {
        return;
    }
    let request = RememberRequest {
        seat: act.seat.to_owned(),
        conversation: memory.conversation(),
        through: session.read_through,
        entries,
    };
    let started = Instant::now();
    let outcome = block_on(memory.remember(&request));
    let latency_ms = elapsed_ms(started);
    let written = match outcome {
        Ok(()) => request.entries.len(),
        Err(error) => {
            failed(env, act.seat, "remember", "activation", &error);
            0
        }
    };
    env.tracer.emit(TraceEvent::Remembered {
        seat: act.seat.to_owned(),
        entries: count(written),
        latency_ms,
    });
}

/// Report every background job a memory finished with.
pub fn finish(env: &Env<'_>) {
    if let Some(memory) = env.memory {
        for line in memory.finish() {
            env.tracer.emit(TraceEvent::Mark {
                label: "memory".into(),
                detail: format!("run: {line}"),
            });
        }
    }
}

/// The `memory` mark for a failed call: its reason and source chain.
fn failed(env: &Env<'_>, seat: &str, op: &str, moment: &str, error: &dyn std::error::Error) {
    let mut reason = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        reason.push_str(&format!(": {cause}"));
        source = cause.source();
    }
    env.tracer.emit(TraceEvent::Mark {
        label: "memory".into(),
        detail: format!("{seat}: {op} {moment} error={reason}"),
    });
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// A ledger line for `cmd`: its exit code and one line of `output`, the last
/// non-empty one (where errors usually are).
pub(super) fn entry(cmd: &str, exit: Option<i32>, output: &str) -> LedgerEntry {
    let line = output
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("(no output)");
    let outcome = if line.chars().count() > LEDGER_OUTCOME {
        let head: String = line.chars().take(LEDGER_OUTCOME).collect();
        format!("{head}...")
    } else {
        line.to_owned()
    };
    LedgerEntry {
        cmd: truncate(cmd, LEDGER_CMD).replace('\n', " "),
        exit,
        outcome,
    }
}
