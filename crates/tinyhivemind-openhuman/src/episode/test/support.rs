//! Fixtures: a scripted runner, journals over the in-memory log, and the
//! desk every test opens.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};
use tinyhivemind::desk::{Desk, ResponderMode};
use tinyhivemind::responder::Probability;
use tinyhivemind::{Conversation, Sequence, SessionFuture, SessionLog};
use tinyhivemind_driver::ConductorState;
use tinyhivemind_driver::{
    AgentBinding, BoundAgent, BoundHive, Commit, Door, EpisodeBrief, Event, HiveGraph, Note,
};
use tinyhivemind_embed::{RouteCandidate, RoutingPolicy};
use tinyhivemind_tools::{EpisodeTools, Refusal};

use super::super::{Journal, Released};
use crate::MemoryLog;
use crate::Result;
use crate::runner::{Lane, SeatRunner, TurnJob, TurnResult};

/// A seat with nothing behind it.
#[derive(Clone, Debug)]
pub(super) struct Seat(String);

impl BoundAgent for Seat {
    fn runtime_id(&self) -> &str {
        &self.0
    }
}

/// One scripted call: a tool by its served name, and its arguments.
pub(super) type Call = (&'static str, Value);

/// A runner whose seats say what they were told to, turn by turn, straight
/// into the record: what a model would do, without one.
pub(super) struct ScriptRunner {
    tools: Arc<EpisodeTools>,
    seats: Vec<String>,
    script: Mutex<BTreeMap<String, VecDeque<Vec<Call>>>>,
    /// Every prompt a seat was sent, in order.
    prompts: Mutex<Vec<(String, Lane, String)>>,
    /// The watermark each turn was opened with, in order.
    since: Mutex<Vec<Option<Sequence>>>,
}

impl ScriptRunner {
    pub(super) fn new(seats: &[&str], script: &[(&str, Vec<Vec<Call>>)]) -> Self {
        Self {
            tools: Arc::new(EpisodeTools::new(seats.iter().map(|id| (*id).to_owned()))),
            seats: seats.iter().map(|id| (*id).to_owned()).collect(),
            script: Mutex::new(
                script
                    .iter()
                    .map(|(seat, turns)| ((*seat).to_owned(), turns.clone().into()))
                    .collect(),
            ),
            prompts: Mutex::new(Vec::new()),
            since: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn prompts(&self) -> Vec<(String, Lane, String)> {
        self.prompts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(super) fn since(&self) -> Vec<Option<Sequence>> {
        self.since
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl SeatRunner for ScriptRunner {
    fn tools(&self) -> &Arc<EpisodeTools> {
        &self.tools
    }

    type Bound = Seat;

    fn bindings(&self) -> Vec<AgentBinding<Seat>> {
        self.seats
            .iter()
            .map(|id| AgentBinding::new(id.clone(), Seat(id.clone())))
            .collect()
    }

    fn turn(&self, seat: String, lane: Lane, since: Option<Sequence>, prompt: String) -> TurnJob {
        self.prompts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((seat.clone(), lane, prompt));
        self.since
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(since);
        let calls = self
            .script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(&seat)
            .and_then(VecDeque::pop_front)
            .unwrap_or_default();
        let tools = Arc::clone(&self.tools);
        Box::pin(async move {
            if calls.iter().any(|(name, _)| *name == "fail") {
                return (seat, lane, TurnResult::Failed("the model went away".into()));
            }
            let parked = calls.iter().any(|(name, _)| *name == "park");
            assert!(
                !calls.iter().any(|(name, _)| *name == "panic"),
                "the model's task panicked"
            );
            for (name, arguments) in &calls {
                let _ = tools.call(&seat, name, arguments);
            }
            if parked {
                return (seat, lane, TurnResult::Parked);
            }
            (seat, lane, TurnResult::Replied("said".into()))
        })
    }
}

/// What the journal saw of one turn: seat, lane, outcome, refusals,
/// recorded calls.
pub(super) type Seen = (String, Lane, TurnResult, usize, usize);

/// A journal over the memory log that keeps what it was shown.
pub(super) struct TestJournal {
    pub(super) log: MemoryLog,
    pub(super) events: Mutex<Vec<Event>>,
    pub(super) turns: Mutex<Vec<Seen>>,
    /// Each desk brief's conversations, by seat, as composed.
    pub(super) shown: Mutex<Vec<(String, usize)>>,
    /// The channels the host names for every seat.
    pub(super) channels: Mutex<Vec<Conversation>>,
    /// Seats the host releases the next time it is asked, then nothing.
    pub(super) release: Mutex<VecDeque<Vec<String>>>,
    /// Every set of parked seats the loop asked about.
    pub(super) asked: Mutex<Vec<Vec<String>>>,
    /// Every snapshot the loop handed over, in order.
    pub(super) checkpoints: Mutex<Vec<ConductorState>>,
}

impl TestJournal {
    pub(super) fn new() -> Self {
        Self::over(MemoryLog::new("engineering"))
    }

    pub(super) fn over(log: MemoryLog) -> Self {
        Self {
            log,
            events: Mutex::new(Vec::new()),
            turns: Mutex::new(Vec::new()),
            shown: Mutex::new(Vec::new()),
            channels: Mutex::new(Vec::new()),
            release: Mutex::new(VecDeque::new()),
            asked: Mutex::new(Vec::new()),
            checkpoints: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn events(&self) -> Vec<Event> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Journal for TestJournal {
    fn log(&self) -> &dyn SessionLog {
        &self.log
    }

    fn checkpoint(&self, state: &ConductorState) -> Result<()> {
        self.checkpoints
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(state.clone());
        Ok(())
    }

    fn channels(&self, _seat: &str) -> Vec<Conversation> {
        self.channels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn released<'a>(&'a self, parked: &'a [String]) -> Released<'a> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(parked.to_vec());
        let released = self
            .release
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or_default();
        Box::pin(async move { Ok(released) })
    }

    fn commit(&self, commit: &Commit) -> Result<Sequence> {
        Ok(self.log.append(
            &commit.author,
            commit.utterance.message(),
            commit.thread,
            &commit.only_for,
        ))
    }

    fn note(&self, note: &Note) -> Result<()> {
        self.log
            .append("desk", &note.body, note.thread, note.only_for.as_slice());
        Ok(())
    }

    fn event(&self, event: &Event) {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event.clone());
    }

    fn compose(&self, seat: &str, brief: &EpisodeBrief) -> String {
        self.shown
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((seat.to_owned(), brief.conversations.len()));
        format!("[{seat}]\n{}", brief.render())
    }

    fn turn_done(
        &self,
        seat: &str,
        lane: Lane,
        outcome: &TurnResult,
        refused: &[Refusal],
        recorded: usize,
    ) {
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((
                seat.to_owned(),
                lane,
                outcome.clone(),
                refused.len(),
                recorded,
            ));
    }
}

pub(super) fn probability(parts: u32) -> Probability {
    Probability::new(parts).expect("bounded")
}

pub(super) fn policy() -> RoutingPolicy {
    RoutingPolicy {
        minimum_confidence: probability(350_000),
        high_impact_minimum_confidence: probability(800_000),
        clarification_threshold: probability(850_000),
        high_impact_threshold: probability(700_000),
        round_width: 1,
        choice_option_limit: 8,
    }
}

pub(super) fn hive(ids: &[&str]) -> BoundHive<Seat> {
    BoundHive::new(
        HiveGraph::new(
            Desk {
                id: "engineering".into(),
                name: "Engineering".into(),
                description: None,
                members: ids.iter().map(|id| (*id).into()).collect(),
                responder_mode: ResponderMode::Auto,
            },
            ids.iter()
                .map(|id| RouteCandidate {
                    id: (*id).into(),
                    label: (*id).into(),
                    role: None,
                    description: None,
                    capabilities: Vec::new(),
                    learned_topics: Vec::new(),
                    available: true,
                })
                .collect(),
        ),
        ids.iter()
            .map(|id| AgentBinding::new(*id, Seat((*id).to_owned())))
            .collect(),
    )
    .expect("hive")
}

pub(super) fn door(journal: &TestJournal, ids: &[&str], starters: &[&str]) -> Door {
    let opened_at = journal
        .log
        .append("operator", "state the root cause", None, &[]);
    Door {
        chat: "engineering".into(),
        desk_name: "Engineering".into(),
        members: ids.iter().map(|id| (*id).into()).collect(),
        starters: starters.iter().map(|id| (*id).into()).collect(),
        opened_at,
    }
}

pub(super) fn complete(message: &str, parent: Option<u64>) -> Call {
    (
        "complete_episode",
        json!({"message": message, "chat": "engineering", "parent": parent.map(|p| p.to_string())}),
    )
}

pub(super) fn ask(to: &str, message: &str, parent: Option<u64>) -> Call {
    (
        "ask",
        json!({"to": to, "message": message, "chat": "engineering", "parent": parent.map(|p| p.to_string())}),
    )
}

pub(super) fn post(message: &str, parent: u64) -> Call {
    (
        "post",
        json!({"message": message, "chat": "engineering", "parent": parent.to_string()}),
    )
}

pub(super) fn run<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(future)
}

pub(super) struct BareJournal(pub(super) MemoryLog);

impl Journal for BareJournal {
    fn log(&self) -> &dyn SessionLog {
        &self.0
    }

    fn commit(&self, commit: &Commit) -> Result<Sequence> {
        Ok(self.0.append(
            &commit.author,
            commit.utterance.message(),
            commit.thread,
            &commit.only_for,
        ))
    }

    fn note(&self, note: &Note) -> Result<()> {
        self.0
            .append("desk", &note.body, note.thread, note.only_for.as_slice());
        Ok(())
    }
}

/// A log that grows under the loop: a host row lands right after a wave's
/// watermark is read, once.
pub(super) struct GrowingLog {
    pub(super) inner: MemoryLog,
    pub(super) late: AtomicBool,
}

impl SessionLog for GrowingLog {
    fn read_before(&self, before: Option<Sequence>, limit: usize) -> SessionFuture<'_> {
        let page = self.inner.read_before(before, limit);
        Box::pin(async move {
            let page = page.await?;
            if before.is_none() && limit == 1 && self.late.swap(false, Ordering::SeqCst) {
                self.inner.append("operator", "one more thing", None, &[]);
            }
            Ok(page)
        })
    }
}

impl Journal for GrowingLog {
    fn log(&self) -> &dyn SessionLog {
        self
    }

    fn commit(&self, commit: &Commit) -> Result<Sequence> {
        Ok(self.inner.append(
            &commit.author,
            commit.utterance.message(),
            commit.thread,
            &commit.only_for,
        ))
    }

    fn note(&self, note: &Note) -> Result<()> {
        self.inner
            .append("desk", &note.body, note.thread, note.only_for.as_slice());
        Ok(())
    }
}
