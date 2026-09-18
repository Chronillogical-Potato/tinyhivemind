//! Concurrent OpenHuman turn preparation and execution for one frozen round.

use std::collections::BTreeSet;
use std::path::PathBuf;

use openhuman_embed::Agent;
use tokio::time::timeout;

use super::{
    DeskMessage, RESEARCH_POLICY, SEALED, TURN_TIMEOUT, Visibility, hive_tools,
    workspace_support::{PendingSnapshot, TurnSnapshots},
};

pub(super) struct TurnContext<'a> {
    pub(super) transcript: &'a [DeskMessage],
    pub(super) visibility: &'a Visibility,
    pub(super) outbox: PathBuf,
    pub(super) assignment: String,
    pub(super) task: &'a str,
    pub(super) prior_failure: &'a str,
    pub(super) problem: &'a str,
}

pub(super) struct PreparedSeatTurn {
    agent: Agent,
    id: String,
    outbox: PathBuf,
    prompt: String,
    session_id: String,
    snapshot: PendingSnapshot,
}

pub(super) struct CompletedSeatTurn {
    pub(super) id: String,
    pub(super) snapshot: PendingSnapshot,
    pub(super) reply: String,
    pub(super) utterances: Vec<tinyhivemind::speech::Utterance>,
}

pub(super) fn prepare_seat_turn(
    agent: Agent,
    id: &str,
    context: TurnContext<'_>,
    snapshots: &mut TurnSnapshots,
) -> anyhow::Result<PreparedSeatTurn> {
    hive_tools::clear(&context.outbox)?;
    let first = context
        .visibility
        .seen
        .get(id)
        .is_none_or(BTreeSet::is_empty);
    let delta = context.visibility.delta(id, context.transcript);
    let policy = if id == "researcher" {
        RESEARCH_POLICY
    } else {
        SEALED
    };
    let prompt = format!(
        "{}{}\n\n## New desk messages\n{}\n\n## This assignment\n{}\n\nThe durable shared workspace is `{}`. Read `AGENTS.md` and `MEMORY.md` before working. Write role-prefixed artifacts there and update `MEMORY.md` only with reproduced, evidence-linked learnings. Do not write or read `/tmp/openhuman` or any other directory.\n\nYou MUST end this turn with exactly one TinyHiveMind action through `mcp_call_tool` on server `tinyhive`: call remote tool `broadcast` with a self-contained message when another teammate should take work, or `complete_episode` with your evidence-dense final result when your assignment is done. First use `mcp_list_tools` if needed. Text outside that MCP call is private thinking and is not delivered to the team.",
        if first {
            format!(
                "## Official statement\n{}\n\n## Prior experiment status\n{}\n\n",
                context.task, context.prior_failure
            )
        } else {
            String::new()
        },
        policy,
        if delta.is_empty() { "(none)" } else { &delta },
        context.assignment,
        agent.action_dir().display(),
    );
    let session_id = format!(
        "tinyhivemind-pe{}-run-{}:{id}",
        context.problem,
        std::process::id()
    );
    let snapshot = snapshots.begin(id, agent.id(), &session_id, &prompt)?;
    Ok(PreparedSeatTurn {
        agent,
        id: id.to_owned(),
        outbox: context.outbox,
        prompt,
        session_id,
        snapshot,
    })
}

pub(super) async fn seat_turn(prepared: PreparedSeatTurn) -> anyhow::Result<CompletedSeatTurn> {
    let outcome = timeout(
        TURN_TIMEOUT,
        prepared
            .agent
            .turn(prepared.prompt)
            .session(&prepared.session_id)
            .send(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("@{} timed out", prepared.id))??;
    println!(
        "[completed] @{}: {}",
        prepared.id,
        outcome.reply.chars().take(500).collect::<String>()
    );
    Ok(CompletedSeatTurn {
        id: prepared.id,
        snapshot: prepared.snapshot,
        reply: outcome.reply,
        utterances: hive_tools::drain(&prepared.outbox)?,
    })
}
