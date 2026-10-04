//! Compaction: the only thing that removes messages from a seat's session.
//!
//! Fires after a call whose prompt went over the activation's budget (and at
//! the top of a resumed activation whose session was left over budget). Every
//! firing is a `context` mark. When summarizing drops messages and memory is
//! on, the commands not yet stored are handed to memory first, then a
//! compaction recall steered by what was dropped is kept right after the
//! opening message.

use serde_json::json;
use tinyhivemind_core::telemetry::TraceEvent;

use super::super::context::{self, Policy};
use super::super::session::SeatSession;
use super::{Activation, Env, Outcome, Work, recall};
use tinyhivemind_core::runtime::RecallMoment;

/// Shrink `session` under the activation's policy after a call that reported
/// a prompt of `prompt` tokens.
pub(super) fn apply(
    env: &Env<'_>,
    act: &Activation<'_>,
    session: &mut SeatSession,
    out: &mut Outcome,
    work: &mut Work,
    prompt: u64,
) {
    let settings = act.context;
    let messages = &mut session.messages;
    let detail = match settings.policy {
        Policy::None => return,
        Policy::Mask => {
            let n = context::mask_observations(messages, settings.keep_recent);
            if n == 0 {
                return;
            }
            format!("masked {n} tool results")
        }
        Policy::Summarize => match summarize(env, act, session, out, work) {
            Some(removed) => format!("summarized {removed} messages"),
            None => return,
        },
        Policy::MaskThenSummarize => {
            let before = context::text_chars(messages);
            let masked = context::mask_observations(messages, settings.keep_recent);
            let estimate = context::scaled_estimate(prompt, before, context::text_chars(messages));
            let summarized = if masked == 0 || estimate > settings.budget {
                summarize(env, act, session, out, work)
            } else {
                None
            };
            match (masked, summarized) {
                (0, None) => return,
                (n, None) => format!("masked {n} tool results (estimate {estimate})"),
                (0, Some(removed)) => format!("summarized {removed} messages"),
                (n, Some(removed)) => {
                    format!("masked {n} tool results, then summarized {removed} messages")
                }
            }
        }
    };
    let detail = format!(
        "{}: {detail} (prompt {prompt} > budget {}, policy {})",
        act.seat,
        settings.budget,
        settings.policy.name()
    );
    env.llm.meter().note_context_event();
    env.tracer.emit(TraceEvent::Mark {
        label: "context".into(),
        detail,
    });
}

/// One extra metered call that condenses the oldest half of the session.
/// Returns how many messages it replaced, or `None` if there was nothing to
/// summarize or the call failed (the session is then left as it was).
fn summarize(
    env: &Env<'_>,
    act: &Activation<'_>,
    session: &mut SeatSession,
    out: &Outcome,
    work: &mut Work,
) -> Option<usize> {
    let messages = &mut session.messages;
    let cut = context::summary_cut(messages)?;
    let log = context::render_for_summary(&messages[2..cut]);
    let ask = [
        json!({ "role": "system", "content": context::SUMMARIZER_SYSTEM }),
        json!({ "role": "user", "content": log }),
    ];
    let done = env.llm.complete(act.seat, &ask, &[]).ok()?;
    if done.content.trim().is_empty() {
        return None;
    }
    let dropped = context::dropped_text(&messages[2..cut]);
    let removed = context::replace_prefix(messages, cut, done.content.trim());
    if env.memory.is_some() {
        recall::remember(env, act, session, out, work);
        if let Some(block) = recall::ask(env, act, RecallMoment::Compaction { dropped }) {
            context::upsert_memory(&mut session.messages, &block);
        }
    }
    Some(removed)
}
