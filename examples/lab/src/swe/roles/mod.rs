//! Prompts: the four hive roles and the single-agent baseline.
//!
//! The environment paragraph is shared by every prompt so the arms differ only
//! in how the work is divided, not in what the model is told about the
//! sandbox. Prompts are deliberately short: they are paid for on every call.

/// A seat's job on the desk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    /// Plans, delegates, decides when the task is done.
    Lead,
    /// Edits the code.
    Implementer,
    /// Runs and writes tests, reports evidence.
    Tester,
    /// Reads the diff for faults the tests miss.
    Reviewer,
}

impl Role {
    /// The four roles in desk order; the lead is first.
    pub const ALL: [Self; 4] = [Self::Lead, Self::Implementer, Self::Tester, Self::Reviewer];

    /// The seat id, which is also what teammates `@mention`.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Implementer => "implementer",
            Self::Tester => "tester",
            Self::Reviewer => "reviewer",
        }
    }

    /// The role whose id is `id`.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.id() == id)
    }

    const fn duty(self) -> &'static str {
        match self {
            Self::Lead => {
                "You are the lead. Read the task, inspect the repo briefly, then split the \
                 work: `broadcast` one concrete job per call (or `ask` a named seat). Pin the \
                 facts the others must not lose by putting a line `!pin #label` in a post. \
                 When the tester reports the task's checks passing and the change is \
                 sound, call `complete_episode` with a short summary. Do not do large edits \
                 yourself."
            }
            Self::Implementer => {
                "You are the implementer. Make the smallest correct change that satisfies the \
                 task, using bash to read and edit files. Re-run what you changed. Report the \
                 files you touched and the key decision in one short `post`."
            }
            Self::Tester => {
                "You are the tester. Run the project's tests and the commands the task names; \
                 write a missing check if needed. Report evidence, the command and the \
                 result, in one short `post`. Do not edit non-test code."
            }
            Self::Reviewer => {
                "You are the reviewer. Read the change (git diff or the files) for bugs the \
                 tests miss, such as edge cases and regressions. Report concrete defects or \
                 say it is sound, in one short `post`. Do not edit code."
            }
        }
    }
}

const ENVIRONMENT: &str = "Tools: `bash` runs commands in a shared sandbox with no internet; \
files you change there are visible to everyone. Keep command output small (use head, tail, \
grep). ";

const HIVE_RULES: &str = "You share a desk with teammates. What you write outside a tool call \
reaches nobody. End each turn by calling exactly one of post, broadcast, ask or \
complete_episode; your next turn starts from the desk, not from this conversation, so put \
everything a teammate needs in that message. Be brief.";

/// The system prompt for a hive seat.
#[must_use]
pub fn hive_system(role: Role, task: &str) -> String {
    format!(
        "{ENVIRONMENT}{HIVE_RULES}\n\n{}\n\nTask:\n{task}",
        role.duty()
    )
}

/// The system prompt for the single-agent baseline.
#[must_use]
pub fn single_system(task: &str) -> String {
    format!(
        "You are a software engineering agent working alone. {ENVIRONMENT}Work until the task \
         is done and verified, then call `complete_episode` with a short summary. Be brief.\n\n\
         Task:\n{task}"
    )
}

/// The opening user message of a hive activation.
#[must_use]
pub fn hive_turn(seat: &str, briefing: &str, reason: &str) -> String {
    format!("{briefing}\n(You are @{seat}. {reason})")
}

/// The opening user message of the single agent.
#[must_use]
pub const fn single_turn() -> &'static str {
    "Begin."
}

#[cfg(test)]
mod test;
