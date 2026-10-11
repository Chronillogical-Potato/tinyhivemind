//! One seat turn: the prompt the seat reads, the recall its binding asks
//! for, the belt it is handed, and the one hive action it ends with.
//!
//! Sessions continue across a seat's turns. The host owns compaction: when a
//! session's context passes [`COMPACT_AT_TOKENS`], the seat's next turn opens a
//! fresh session and recalls with `RecallMoment::Compaction`, the dropped
//! journal rows as the query, so the binding's third recall moment is real.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use openhuman_embed::{Agent, AgentProgress, HostTurnTools};
use tinyhivemind_core::runtime::{
    EntryKind, Recall, RecallMoment, RecallRequest, Remember, RememberEntry, RememberRequest,
    frame_recalled,
};

use super::floors::{Assignment, Row};
use super::memory::SeatMemory;
use super::package::SeatTool;
use super::tools::{Action, CallRecord, SeatScope, Services, TurnLedger, belt};

/// A session past this many context tokens is compacted by the host.
pub(super) const COMPACT_AT_TOKENS: u64 = 60_000;
/// One turn's wall-clock limit.
pub(super) const TURN_TIMEOUT: Duration = Duration::from_secs(900);
/// Characters of one journal row a seat reads.
const ROW_CHARS: usize = 3_000;

/// A seat's continuing session.
#[derive(Debug, Default)]
struct Session {
    generation: u32,
    turns: u32,
    context_tokens: u64,
    compacted: Vec<String>,
}

/// One seat as the host runs it.
pub(super) struct Seat {
    pub(super) agent: Agent,
    pub(super) tools: BTreeSet<SeatTool>,
    pub(super) memory: Arc<SeatMemory>,
    pub(super) scope: SeatScope,
    session: Mutex<Session>,
}

/// Everything a turn needs that is not the seat.
pub(super) struct TurnInput {
    pub(super) problem: String,
    pub(super) desk_line: String,
    pub(super) rows: Vec<Row>,
    pub(super) assignment: Assignment,
    pub(super) budget_line: String,
}

/// What one turn produced.
#[derive(Debug)]
pub(super) struct TurnOutput {
    pub(super) seat: String,
    pub(super) action: Action,
    pub(super) recovered: bool,
    pub(super) reply: String,
    pub(super) prompt: String,
    pub(super) calls: Vec<CallRecord>,
    pub(super) recalled: usize,
    pub(super) remembered: usize,
    pub(super) model_calls: u64,
    pub(super) input_tokens: u64,
    pub(super) output_tokens: u64,
    pub(super) cost_usd: Option<f64>,
    pub(super) moment: &'static str,
    pub(super) error: Option<String>,
}

impl Seat {
    pub(super) fn new(
        agent: Agent,
        tools: BTreeSet<SeatTool>,
        memory: Arc<SeatMemory>,
        scope: SeatScope,
    ) -> Self {
        Self {
            agent,
            tools,
            memory,
            scope,
            session: Mutex::new(Session::default()),
        }
    }

    fn session_id(&self, run: &str, generation: u32) -> String {
        format!("euler-{run}-{}-g{generation}", self.scope.seat)
    }

    /// The recall moment this turn opens with, and whether the session is new.
    fn moment(&self) -> (RecallMoment, bool, u32) {
        let session = self.session.lock().map(|s| (s.generation, s.turns, s.compacted.clone()));
        let (generation, turns, compacted) = session.unwrap_or((0, 0, Vec::new()));
        match (generation, turns) {
            (0, 0) => (RecallMoment::SessionStart, true, generation),
            (_, 0) => (RecallMoment::Compaction { dropped: compacted }, true, generation),
            _ => (RecallMoment::Rejoin, false, generation),
        }
    }
}

/// The turn prompt. Pure, so its shape is pinned by test.
pub(super) fn prompt(input: &TurnInput, fresh: bool, recalled: Option<&str>) -> String {
    let mut out = String::new();
    if fresh {
        out.push_str("## The problem\n");
        out.push_str(input.problem.trim());
        out.push_str("\n\n");
    }
    out.push_str("## Your desk\n");
    out.push_str(&input.desk_line);
    out.push_str("\n\n");
    if let Some(recalled) = recalled {
        out.push_str(recalled.trim());
        out.push_str("\n\n");
    }
    out.push_str("## New on your desk since your last turn\n");
    if input.rows.is_empty() {
        out.push_str("(nothing new)\n");
    }
    for row in &input.rows {
        let body: String = row.body.chars().take(ROW_CHARS).collect();
        out.push_str(&format!("[{}] @{} ({}): {body}\n", row.sequence, row.author, row.kind));
    }
    out.push_str("\n## Why you are running\n");
    let note: String = input.assignment.note.chars().take(ROW_CHARS).collect();
    out.push_str(&format!("{}: {note}\n\n", input.assignment.reason));
    out.push_str("## Budget left\n");
    out.push_str(&input.budget_line);
    out.push_str("\n\nEnd this turn with exactly one of hive_complete, hive_broadcast or hive_refer.");
    out
}

const NUDGE: &str = "You ended without a hive action, so nothing reached your desk. Call exactly one of hive_complete, hive_broadcast or hive_refer now, with your result so far.";

/// Run one turn of `seat`, retrying once when it takes no hive action.
pub(super) async fn run(seat: Arc<Seat>, input: TurnInput, services: Arc<Services>) -> TurnOutput {
    let (moment, fresh, generation) = seat.moment();
    let label = moment.label();
    let focus = format!(
        "{} {}",
        input.desk_line,
        input.problem.chars().take(400).collect::<String>()
    );
    let budget = seat.memory.binding().settings.budget_chars;
    let request = RecallRequest {
        seat: seat.scope.seat.clone(),
        conversation: seat.scope.conversation.clone(),
        focus: Some(focus),
        moment,
        budget_chars: budget,
    };
    let (framed, recalled) = match seat.memory.recall(&request).await {
        Ok(notes) => {
            let count = notes.iter().map(|note| note.lines.len()).sum();
            (frame_recalled(&notes, budget), count)
        }
        Err(error) => {
            services.run.event(&serde_json::json!({"event": "recall_failed", "seat": seat.scope.seat, "error": error.to_string()}));
            (None, 0)
        }
    };
    let text = prompt(&input, fresh, framed.as_deref());
    let ledger = Arc::new(TurnLedger::default());
    ledger.add_recalled(recalled);
    let session = seat.session_id(&services.run_id, generation);
    let mut output = TurnOutput {
        seat: seat.scope.seat.clone(),
        action: Action::Complete {
            report: String::new(),
            candidate: None,
            verdict: None,
        },
        recovered: false,
        reply: String::new(),
        prompt: text.clone(),
        calls: Vec::new(),
        recalled,
        remembered: 0,
        model_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        cost_usd: None,
        moment: label,
        error: None,
    };
    let mut message = text;
    for attempt in 0..2 {
        match one_turn(&seat, &message, &session, &ledger, &services).await {
            Ok((reply, usage, calls)) => {
                output.model_calls += calls;
                if let Some(usage) = &usage {
                    output.input_tokens += usage.input_tokens;
                    output.output_tokens += usage.output_tokens;
                    output.cost_usd = Some(output.cost_usd.unwrap_or(0.0) + usage.cost_usd.unwrap_or(0.0));
                    if let Ok(mut s) = seat.session.lock() {
                        s.context_tokens = usage.context_tokens;
                    }
                }
                output.reply = reply;
            }
            Err(error) => {
                output.error = Some(error.to_string());
                break;
            }
        }
        if ledger.action().is_some() || attempt == 1 {
            break;
        }
        message = NUDGE.into();
    }
    match ledger.action() {
        Some(action) => output.action = action,
        None => {
            output.recovered = true;
            let report: String = if output.reply.trim().is_empty() {
                output
                    .error
                    .clone()
                    .unwrap_or_else(|| "(the seat produced nothing)".into())
            } else {
                output.reply.chars().take(4_000).collect()
            };
            output.action = Action::Complete {
                report: format!("[host: no hive action taken; reply recorded] {report}"),
                candidate: None,
                verdict: None,
            };
        }
    }
    output.calls = ledger.calls();
    let (_, remembered) = ledger.memory();
    output.remembered = remembered + remember_action(&seat, &output.action, &services).await;
    if let Ok(mut s) = seat.session.lock() {
        s.turns += 1;
        if s.context_tokens > COMPACT_AT_TOKENS {
            s.generation += 1;
            s.turns = 0;
            s.compacted = input
                .rows
                .iter()
                .map(|row| row.body.chars().take(400).collect())
                .collect();
            services.run.event(&serde_json::json!({"event": "compaction", "seat": seat.scope.seat, "context_tokens": s.context_tokens}));
        }
    }
    output
}

async fn one_turn(
    seat: &Seat,
    message: &str,
    session: &str,
    ledger: &Arc<TurnLedger>,
    services: &Arc<Services>,
) -> anyhow::Result<(String, Option<openhuman_embed::LastTurnUsage>, u64)> {
    let tools = seat.tools.clone();
    let scope = seat.scope.clone();
    let turn_ledger = ledger.clone();
    let shared = services.clone();
    let memory = seat.memory.clone();
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<AgentProgress>(4_096);
    let counter = tokio::spawn(async move {
        let mut calls = 0_u64;
        while let Some(event) = receiver.recv().await {
            if matches!(event, AgentProgress::IterationStarted { .. }) {
                calls += 1;
            }
        }
        calls
    });
    let outcome = seat
        .agent
        .turn(message.to_owned())
        .session(session)
        .tools(move |_| HostTurnTools::advertised(belt(&tools, &scope, &turn_ledger, &shared, &memory)))
        .on_progress(sender)
        .timeout(TURN_TIMEOUT)
        .send()
        .await;
    let calls = counter.await.unwrap_or(0);
    services
        .budget
        .model_calls
        .fetch_add(calls, std::sync::atomic::Ordering::Relaxed);
    let outcome = outcome?;
    services.budget.record_turn(outcome.usage.as_ref());
    Ok((outcome.reply, outcome.usage, calls))
}

/// Core's `Remember` after each activation: the seat's committed action as
/// one entry of the kind its binding allows.
async fn remember_action(seat: &Seat, action: &Action, services: &Services) -> usize {
    let (kind, text) = match action {
        Action::Complete {
            report, candidate, ..
        } => (
            EntryKind::Outcome,
            format!(
                "{}{}",
                candidate
                    .as_deref()
                    .map(|candidate| format!("candidate {candidate}: "))
                    .unwrap_or_default(),
                report.chars().take(1_200).collect::<String>()
            ),
        ),
        Action::Broadcast { to, message } => (
            EntryKind::Note,
            format!("handed @{to}: {}", message.chars().take(600).collect::<String>()),
        ),
        Action::Refer { desk, message } => (
            EntryKind::Note,
            format!("asked #{desk}: {}", message.chars().take(600).collect::<String>()),
        ),
    };
    let request = RememberRequest {
        seat: seat.scope.seat.clone(),
        conversation: seat.scope.conversation.clone(),
        through: None,
        entries: vec![RememberEntry { kind, text }],
    };
    match seat.memory.remember(&request).await {
        Ok(()) => 1,
        Err(error) => {
            services.run.event(&serde_json::json!({"event": "remember_failed", "seat": seat.scope.seat, "error": error.to_string()}));
            0
        }
    }
}
