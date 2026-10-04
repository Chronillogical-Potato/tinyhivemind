//! Where an activation meets its session and memory.
//!
//! [`open`] starts or resumes the session; [`ask`] and [`remember`] are the
//! two memory calls, each reported as a `memory` mark; [`entry`] turns one
//! executed command into a ledger line.

use serde_json::json;
use tinyhivemind_core::telemetry::TraceEvent;

use super::super::memory::{LedgerEntry, Moment, Remembered, Report};
use super::super::sandbox::truncate;
use super::super::session::SeatSession;
use super::{Activation, Env, Outcome, Work};

/// Longest command kept in a ledger line, in bytes.
const LEDGER_CMD: usize = 200;
/// Longest outcome kept in a ledger line, in characters.
const LEDGER_OUTCOME: usize = 160;

/// Start `session` (system prompt, then the opening with a session-start
/// pack in front) or resume it (the delta, with a rejoin pack after).
pub(super) fn open(env: &Env<'_>, act: &Activation<'_>, session: &mut SeatSession) {
    let focus = act.focus.clone();
    if session.is_new() {
        let pack = ask(env, act.seat, &Moment::SessionStart { focus });
        let user = match pack {
            Some(pack) => format!("{pack}\n\n{}", act.user),
            None => act.user.clone(),
        };
        session
            .messages
            .push(json!({ "role": "system", "content": act.system }));
        session
            .messages
            .push(json!({ "role": "user", "content": user }));
    } else {
        let pack = ask(env, act.seat, &Moment::Rejoin { focus });
        let user = match pack {
            Some(pack) => format!("{}\n\n{pack}", act.user),
            None => act.user.clone(),
        };
        session
            .messages
            .push(json!({ "role": "user", "content": user }));
    }
}

/// Recall a pack for `seat` at `moment`, or `None` without memory or when the
/// call found nothing or failed.
pub(super) fn ask(env: &Env<'_>, seat: &str, moment: &Moment) -> Option<String> {
    let memory = env.memory?;
    let recalled = memory.recall(seat, moment);
    mark(env, seat, &recalled.report);
    recalled.pack
}

/// Hand memory the commands not yet stored and the seat's latest words.
pub(super) fn remember(
    env: &Env<'_>,
    act: &Activation<'_>,
    out: &Outcome,
    work: &mut Work,
    moment: &'static str,
) {
    let Some(memory) = env.memory else { return };
    let text = out
        .spoke
        .as_ref()
        .map_or_else(|| work.last_text.clone(), |done| done.content.clone());
    let what = Remembered {
        text,
        ledger: out.ledger[work.remembered.min(out.ledger.len())..].to_vec(),
    };
    work.remembered = out.ledger.len();
    if what.is_empty() {
        return;
    }
    let mut report = memory.remember(act.seat, &what);
    report.moment = moment;
    mark(env, act.seat, &report);
}

/// Report every background job a memory finished with.
pub fn finish(env: &Env<'_>) {
    if let Some(memory) = env.memory {
        for report in memory.finish() {
            mark(env, "run", &report);
        }
    }
}

fn mark(env: &Env<'_>, seat: &str, report: &Report) {
    env.tracer.emit(TraceEvent::Mark {
        label: "memory".into(),
        detail: report.detail(seat),
    });
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
