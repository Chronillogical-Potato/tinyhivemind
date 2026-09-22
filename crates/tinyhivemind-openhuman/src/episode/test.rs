//! The episode loop over a journal, with a scripted runner and no model.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};
use tinyhivemind::desk::{Desk, ResponderMode};
use tinyhivemind::responder::Probability;
use tinyhivemind::{Sequence, SessionLog};
use tinyhivemind_driver::{
    AgentBinding, BoundAgent, BoundHive, BroadcastRouting, Commit, CompletionDriver, ConductPolicy,
    Door, EpisodeBrief, Event, HiveGraph, Note,
};
use tinyhivemind_embed::{RouteCandidate, RoutingPolicy};
use tinyhivemind_tools::{EpisodeTools, Refusal};

use super::{Journal, Report, run_episode};
use crate::MemoryLog;
use crate::runner::{Lane, SeatRunner, TurnJob, TurnResult};
use crate::{Error, Result};

/// A seat with nothing behind it.
#[derive(Clone, Debug)]
struct Seat(String);

impl BoundAgent for Seat {
    fn runtime_id(&self) -> &str {
        &self.0
    }
}

/// One scripted call: a tool by its served name, and its arguments.
type Call = (&'static str, Value);

/// A runner whose seats say what they were told to, turn by turn, straight
/// into the record: what a model would do, without one.
struct ScriptRunner {
    tools: Arc<EpisodeTools>,
    seats: Vec<String>,
    script: Mutex<BTreeMap<String, VecDeque<Vec<Call>>>>,
    /// Every prompt a seat was sent, in order.
    prompts: Mutex<Vec<(String, Lane, String)>>,
    /// The watermark each turn was opened with, in order.
    since: Mutex<Vec<Option<Sequence>>>,
}

impl ScriptRunner {
    fn new(seats: &[&str], script: &[(&str, Vec<Vec<Call>>)]) -> Self {
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

    fn prompts(&self) -> Vec<(String, Lane, String)> {
        self.prompts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn since(&self) -> Vec<Option<Sequence>> {
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
                return (seat, lane, Some(Err("the model went away".into())));
            }
            assert!(
                !calls.iter().any(|(name, _)| *name == "panic"),
                "the model's task panicked"
            );
            for (name, arguments) in &calls {
                let _ = tools.call(&seat, name, arguments);
            }
            (seat, lane, Some(Ok("said".into())))
        })
    }
}

/// What the journal saw of one turn: seat, lane, outcome, refusals,
/// recorded calls.
type Seen = (String, Lane, TurnResult, usize, usize);

/// A journal over the memory log that keeps what it was shown.
struct TestJournal {
    log: MemoryLog,
    events: Mutex<Vec<Event>>,
    turns: Mutex<Vec<Seen>>,
    /// Each desk brief's conversations, by seat, as composed.
    shown: Mutex<Vec<(String, usize)>>,
}

impl TestJournal {
    fn new() -> Self {
        Self::over(MemoryLog::new("engineering"))
    }

    fn over(log: MemoryLog) -> Self {
        Self {
            log,
            events: Mutex::new(Vec::new()),
            turns: Mutex::new(Vec::new()),
            shown: Mutex::new(Vec::new()),
        }
    }

    fn events(&self) -> Vec<Event> {
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

    fn commit(&self, commit: &Commit) -> Result<Sequence> {
        Ok(self.log.append(
            &commit.author,
            commit.utterance.message(),
            commit.thread,
            commit.only_for.as_deref(),
        ))
    }

    fn note(&self, note: &Note) -> Result<()> {
        self.log
            .append("desk", &note.body, note.thread, note.only_for.as_deref());
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

fn probability(parts: u32) -> Probability {
    Probability::new(parts).expect("bounded")
}

fn policy() -> RoutingPolicy {
    RoutingPolicy {
        minimum_confidence: probability(350_000),
        high_impact_minimum_confidence: probability(800_000),
        clarification_threshold: probability(850_000),
        high_impact_threshold: probability(700_000),
        round_width: 1,
        choice_option_limit: 8,
    }
}

fn hive(ids: &[&str]) -> BoundHive<Seat> {
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

fn door(journal: &TestJournal, ids: &[&str], starters: &[&str]) -> Door {
    let opened_at = journal
        .log
        .append("operator", "state the root cause", None, None);
    Door {
        chat: "engineering".into(),
        desk_name: "Engineering".into(),
        members: ids.iter().map(|id| (*id).into()).collect(),
        starters: starters.iter().map(|id| (*id).into()).collect(),
        opened_at,
    }
}

fn complete(message: &str, parent: Option<u64>) -> Call {
    (
        "complete_episode",
        json!({"message": message, "chat": "engineering", "parent": parent.map(|p| p.to_string())}),
    )
}

fn ask(to: &str, message: &str, parent: Option<u64>) -> Call {
    (
        "ask",
        json!({"to": to, "message": message, "chat": "engineering", "parent": parent.map(|p| p.to_string())}),
    )
}

fn post(message: &str, parent: u64) -> Call {
    (
        "post",
        json!({"message": message, "chat": "engineering", "parent": parent.to_string()}),
    )
}

fn run<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(future)
}

#[test]
fn an_episode_runs_from_its_door_to_quiescence_over_the_journal() {
    let hive = hive(&["one", "two"]);
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let route_policy = policy();
    let routing = BroadcastRouting {
        primary: None,
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let journal = TestJournal::new();
    // One asks two (row 2 roots the conversation) and, woken by its own ask
    // row, says nothing; two tries a post, which is not served, and answers
    // in the thread with a completion; one, released by the conclusion,
    // completes on the desk.
    let runner = ScriptRunner::new(
        &["one", "two"],
        &[
            (
                "one",
                vec![
                    vec![ask("two", "which port?", None)],
                    vec![],
                    vec![complete("fixed", None)],
                ],
            ),
            (
                "two",
                vec![vec![post("checking", 2), complete("port 8080", Some(2))]],
            ),
        ],
    );
    let report = run(run_episode(
        &journal,
        &runner,
        &driver,
        routing,
        ConductPolicy::default(),
        door(&journal, &["one", "two"], &["one"]),
    ))
    .expect("the episode settles");
    assert_eq!(
        report,
        Report {
            turns: 4,
            waves: 3,
            discharged: 0,
            conversations: 1,
            settled: 2,
        },
        "{:?}",
        journal.log.all()
    );
    let bodies: Vec<String> = journal
        .log
        .all()
        .iter()
        .map(|row| row.body.clone())
        .collect();
    assert_eq!(bodies[0], "state the root cause");
    assert!(bodies.contains(&"which port?".to_owned()));
    assert!(bodies.contains(&"port 8080".to_owned()));
    assert!(
        bodies
            .iter()
            .any(|body| body.contains("concluded our conversation"))
    );
    assert!(bodies.contains(&"fixed".to_owned()));
    let events = journal.events();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Asked { root, .. } if *root == Sequence(2)))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Concluded { forced: false, .. }))
    );

    the_journal_saw_each_turn(&journal, &runner);
}

/// The thread turn was briefed with the thread, the asker's waking desk
/// turn was shown the concluded conversation, and every turn came back to
/// the journal with what it recorded.
fn the_journal_saw_each_turn(journal: &TestJournal, runner: &ScriptRunner) {
    let prompts = runner.prompts();
    let (_, lane, thread_prompt) = prompts
        .iter()
        .find(|(seat, _, _)| seat == "two")
        .expect("two ran");
    assert_eq!(*lane, Lane::Thread(Sequence(2)));
    assert!(thread_prompt.contains("which port?"), "{thread_prompt}");
    let shown = journal.shown.lock().unwrap();
    assert!(
        shown
            .iter()
            .any(|(seat, conversations)| seat == "one" && *conversations == 1)
    );
    // Every turn came back to the journal with what it recorded.
    let turns = journal.turns.lock().unwrap();
    assert_eq!(turns.len(), 4, "three that called, and one's silent turn");
    assert!(
        turns
            .iter()
            .all(|(_, _, outcome, _, _)| matches!(outcome, Some(Ok(_))))
    );
    // `post` is in the vocabulary and not served: two's post in the thread
    // was refused inside its turn, and the journal was told so.
    let refusals: Vec<(&str, usize)> = turns
        .iter()
        .map(|(seat, _, _, refused, _)| (seat.as_str(), *refused))
        .filter(|(_, refused)| *refused > 0)
        .collect();
    assert_eq!(refusals, vec![("two", 1)]);
    assert_eq!(
        turns
            .iter()
            .filter(|(_, _, _, _, recorded)| *recorded >= 1)
            .count(),
        3
    );
}

#[test]
fn a_task_on_a_log_numbered_from_zero_reaches_the_first_turn() {
    let hive = hive(&["one", "two"]);
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let route_policy = policy();
    let routing = BroadcastRouting {
        primary: None,
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    // A host that numbers its first row zero, as some do: the task is row
    // zero, and nothing sits below it.
    let journal = TestJournal::over(MemoryLog::numbered_from("engineering", Sequence(0)));
    let runner = ScriptRunner::new(
        &["one", "two"],
        &[("one", vec![vec![complete("done", None)]])],
    );
    let entrance = door(&journal, &["one", "two"], &["one"]);
    assert_eq!(entrance.opened_at, Sequence(0));
    let report = run(run_episode(
        &journal,
        &runner,
        &driver,
        routing,
        ConductPolicy::default(),
        entrance,
    ))
    .expect("the episode runs");
    assert_eq!(report.settled, 2);
    assert_eq!(report.turns, 1);
    // The seat's one turn was shown the task, and was shown nothing before.
    let prompts = runner.prompts();
    assert_eq!(prompts.len(), 1);
    assert!(
        prompts[0].2.contains("state the root cause"),
        "{}",
        prompts[0].2
    );
    assert_eq!(runner.since(), vec![None]);
    // The completion landed above the task, at row one.
    let rows = journal.log.all();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].sequence, Sequence(1));
    assert!(rows[1].body.contains("done"));
}

#[test]
fn a_seat_that_says_nothing_is_nudged_and_then_the_episode_stalls() {
    let hive = hive(&["one", "two"]);
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let route_policy = policy();
    let routing = BroadcastRouting {
        primary: None,
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let journal = TestJournal::new();
    let runner = ScriptRunner::new(
        &["one", "two"],
        &[("one", vec![vec![], vec![("fail", json!({}))]])],
    );
    let stalled = run(run_episode(
        &journal,
        &runner,
        &driver,
        routing,
        ConductPolicy::default(),
        door(&journal, &["one", "two"], &["one"]),
    ));
    assert!(
        matches!(&stalled, Err(Error::Conduct(tinyhivemind_driver::Error::Stalled { seats })) if seats == &["one".to_owned()]),
        "{stalled:?}"
    );
    // The nudge reached the journal as a row to one alone, and the failed
    // turn reached it as a turn that failed.
    let rows = journal.log.all();
    assert!(
        rows.iter()
            .any(|row| row.author == "desk" && row.only_for.as_deref() == Some("one"))
    );
    let turns = journal.turns.lock().unwrap();
    assert!(
        turns
            .iter()
            .any(|(_, _, outcome, _, recorded)| matches!(outcome, Some(Err(_))) && *recorded == 0)
    );
    assert!(
        journal
            .events()
            .iter()
            .any(|event| matches!(event, Event::Nudged { thread: None, .. }))
    );
    // The nudge is one's alone: it woke nobody else, and what two would be
    // shown of the desk does not hold it.
    let prompts = runner.prompts();
    assert!(
        prompts.iter().all(|(seat, _, _)| seat == "one"),
        "{prompts:?}"
    );
    let shown_two = journal.log.desk_since("two", None);
    assert!(!shown_two.is_empty());
    assert!(
        shown_two.iter().all(|row| !row.contains("open work")),
        "{shown_two:?}"
    );
}

/// A journal that overrides nothing it need not: the log and the appends.
struct BareJournal(MemoryLog);

impl Journal for BareJournal {
    fn log(&self) -> &dyn SessionLog {
        &self.0
    }

    fn commit(&self, commit: &Commit) -> Result<Sequence> {
        Ok(self.0.append(
            &commit.author,
            commit.utterance.message(),
            commit.thread,
            commit.only_for.as_deref(),
        ))
    }

    fn note(&self, note: &Note) -> Result<()> {
        self.0
            .append("desk", &note.body, note.thread, note.only_for.as_deref());
        Ok(())
    }
}

#[test]
fn a_journal_that_keeps_the_defaults_is_briefed_as_the_episode_words_it() {
    let hive = hive(&["one", "two"]);
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let route_policy = policy();
    let routing = BroadcastRouting {
        primary: None,
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let journal = BareJournal(MemoryLog::new("engineering"));
    let opened_at = journal.0.append("operator", "the task", None, None);
    // The first turn's task panics; the second completes. A panicked task
    // is a failed turn, not a failed wave, and the seat runs again.
    let runner = ScriptRunner::new(
        &["one", "two"],
        &[(
            "one",
            vec![vec![("panic", json!({}))], vec![complete("done", None)]],
        )],
    );
    let report = run(run_episode(
        &journal,
        &runner,
        &driver,
        routing,
        ConductPolicy::default(),
        Door {
            chat: "engineering".into(),
            desk_name: "Engineering".into(),
            members: vec!["one".into(), "two".into()],
            starters: vec!["one".into()],
            opened_at,
        },
    ))
    .expect("the episode settles");
    assert_eq!(report.settled, 2);
    assert_eq!(report.conversations, 0);
    // The default composition is the brief alone: the operator's row, as the
    // episode renders it.
    let prompts = runner.prompts();
    assert!(prompts[0].2.starts_with("## "), "{}", prompts[0].2);
    assert!(
        prompts[0].2.contains("@operator: the task"),
        "{}",
        prompts[0].2
    );
    assert_eq!(prompts.len(), 2, "the panicked turn was run again");
    assert!(journal.0.all().iter().any(|row| row.body == "done"));
}
