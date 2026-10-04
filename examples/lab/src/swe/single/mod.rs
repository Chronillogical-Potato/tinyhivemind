//! The baseline arm: one seat, one long conversation.
//!
//! Same model, same `bash` tool, same sandbox, same meter and the same caps as
//! the hive. Its only speaking tool is `complete_episode`, which ends the run.
//! Nothing is pruned: the conversation grows with every command, which is what
//! a conventional agent loop pays and what the hive arm is compared against.

use super::roles::{single_system, single_turn};
use super::seat::{Activation, Env, Outcome, run as run_seat};
use super::tools::{SINGLE_TOOLS, tool_list};

/// The single agent's seat id.
pub const SEAT: &str = "agent";

/// Run the single agent on `task`, allowing up to `steps` model calls.
pub fn run(env: &Env<'_>, task: &str, steps: usize) -> Outcome {
    run_seat(
        env,
        &Activation {
            seat: SEAT,
            system: single_system(task),
            user: single_turn().into(),
            tools: tool_list(SINGLE_TOOLS),
            speaking: SINGLE_TOOLS,
            steps,
            implicit_post: false,
        },
    )
}
