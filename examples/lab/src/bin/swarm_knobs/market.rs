//! Shared transcripts for the mechanism tables, and the order they run in.

use tinyhivemind_core::aside::Audience;
use tinyhivemind_core::runtime::{SessionAuthor, SessionMessage};
use tinyhivemind_lab::{Res, TraceRig, agent, row};

use crate::{attention, consensus, division};

pub fn desk(seq: u64, who: &str, body: &str) -> SessionMessage {
    row(seq, agent(who), body, Audience::Desk)
}

pub fn aside(seq: u64, who: &str, to: &str, body: &str) -> SessionMessage {
    row(
        seq,
        agent(who),
        body,
        Audience::Aside {
            members: vec![to.into()],
        },
    )
}

pub fn journal() -> Vec<SessionMessage> {
    vec![
        row(1, SessionAuthor::Operator, "Pick a plan.", Audience::Desk),
        desk(2, "ada", "!propose #x plan x ^1"),
        desk(3, "ben", "!support #x agree ^2"),
        desk(4, "cy", "!propose #y plan y ^1"),
        desk(5, "ada", "!evidence #x the benchmark holds ^2"),
        desk(6, "di", "!object >2 ^2 that plan is weak"),
        desk(7, "ada", "!support #x again ^2"),
    ]
}


pub fn run(rig: &TraceRig) -> Res {
    attention::attention();
    attention::salience_table()?;
    division::directory_and_division()?;
    consensus::exchange_rounds()?;
    consensus::evaluated_quorum(rig)?;
    consensus::trace_grammar();
    Ok(())
}
