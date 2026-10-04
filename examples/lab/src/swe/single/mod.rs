//! The baseline arm: one seat, one long conversation.
//!
//! Same model, same `bash` tool, same sandbox, same meter and the same caps as
//! the hive. Its only speaking tool is `complete_episode`, which ends the run.
//! What happens when the conversation outgrows the model's context is an
//! explicit, measured choice (`--single-context none|mask|summarize`, see
//! [`context`](super::context)), so the baseline does not die of overflow for
//! reasons unrelated to the hive idea.

use super::context::Settings;
use super::roles::{single_system, single_turn};
use super::seat::{Activation, Env, Outcome, run as run_seat};
use super::tools::{SINGLE_TOOLS, tool_list};

/// The single agent's seat id.
pub const SEAT: &str = "agent";

/// Longest memory focus taken from the task, in characters.
const FOCUS_CHARS: usize = 600;

/// Run the single agent on `task`, allowing up to `steps` model calls.
///
/// Its one activation runs on a session from the run's store, so it goes
/// through the same compaction and memory path as a hive seat.
pub fn run(env: &Env<'_>, task: &str, steps: usize, context: Settings) -> Outcome {
    let mut session = env.sessions.take(SEAT);
    let out = run_seat(
        env,
        &Activation {
            seat: SEAT,
            system: single_system(task),
            user: single_turn().into(),
            shown_rows: 0,
            focus: task.chars().take(FOCUS_CHARS).collect(),
            tools: tool_list(SINGLE_TOOLS),
            speaking: SINGLE_TOOLS,
            steps,
            implicit_post: false,
            context,
        },
        &mut session,
    );
    env.sessions.put(SEAT, session);
    out
}
