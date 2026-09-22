//! The ledger's ordering rules: drain after completion, assign at the
//! completion's sequence, hold completion on an open question, admit a budget
//! before routing, and report the episode over only when quiescent.

use tinyhivemind::{Sequence, responder::Probability, speech::Utterance};
use tinyhivemind_embed::{
    CandidateProbability, EvaluationDisposition, Router, RouterFuture, RoutingEvaluation,
    RoutingRequest,
};

use super::{committed, episode, hive, policy};
use crate::driver::{BroadcastRouting, CompletionDriver, DriverState, HostAction, Transition};
use crate::{Error, Result};

fn run<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime")
        .block_on(future)
}

/// Routes every broadcast to the first candidate, alone.
#[derive(Debug, Default)]
struct FirstRouter {
    calls: std::sync::atomic::AtomicUsize,
}

impl Router for FirstRouter {
    fn evaluate<'a>(&'a self, request: &'a RoutingRequest) -> RouterFuture<'a> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let first = request.candidates[0].id.clone();
        let roster_version = request.roster_version;
        Box::pin(async move {
            Ok(RoutingEvaluation {
                primary_responder: first.clone(),
                primary_probabilities: vec![CandidateProbability {
                    candidate_id: first,
                    probability: Probability::new(900_000).expect("bounded"),
                }],
                confidence: Probability::new(900_000).expect("bounded"),
                needs_collaboration: Probability::new(0).expect("bounded"),
                needs_clarification: Probability::new(0).expect("bounded"),
                contributions: Vec::new(),
                high_impact: Probability::new(0).expect("bounded"),
                model_identity: "first".into(),
                question_schema_version: 1,
                roster_version,
                disposition: EvaluationDisposition::Unchecked,
            })
        })
    }
}

fn broadcast(message: &str) -> Utterance {
    Utterance::Broadcast {
        message: message.into(),
    }
}

fn complete() -> Utterance {
    Utterance::CompleteEpisode {
        message: "done".into(),
    }
}

fn post() -> Utterance {
    Utterance::Post {
        message: "note".into(),
    }
}

fn ask(to: &str) -> Utterance {
    Utterance::Ask {
        to: to.into(),
        message: "is it tight?".into(),
    }
}

fn apply(
    driver: &CompletionDriver<'_>,
    state: &DriverState,
    author: &str,
    sequence: u64,
    utterance: Utterance,
    router: &FirstRouter,
) -> Result<Transition> {
    let route_policy = policy(4);
    let routing = BroadcastRouting {
        primary: Some(router),
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    run(driver.apply_committed(state, committed(author, sequence, utterance), Some(routing)))
}

fn open_at(state: &DriverState, id: &str) -> Option<Sequence> {
    state
        .episode()
        .participants
        .iter()
        .find(|participant| participant.agent_id == id)
        .and_then(|participant| participant.open())
        .map(|record| record.assigned_at)
}

fn round_ids(driver: &CompletionDriver<'_>, state: &DriverState) -> Vec<String> {
    driver
        .pending_round(state)
        .expect("round")
        .agents()
        .iter()
        .map(|pending| pending.hive_agent_id.to_owned())
        .collect()
}

#[test]
fn a_completion_drains_one_queued_handoff_and_assigns_it_at_the_completion() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let router = FirstRouter::default();
    let state = driver.start(episode(&["one", "two"])).expect("state");
    // Everyone opened is still working, so the handoff is held, not given.
    let after_broadcast = apply(&driver, &state, "one", 1, broadcast("take this"), &router)
        .expect("broadcast")
        .state;
    assert_eq!(after_broadcast.ledger().queue_len("two"), 1);
    assert_eq!(
        open_at(&after_broadcast, "two"),
        Some(Sequence(0)),
        "a working recipient keeps the assignment it holds",
    );

    let transition =
        apply(&driver, &after_broadcast, "two", 2, complete(), &router).expect("completion drains");
    assert_eq!(transition.state.ledger().queue_len("two"), 0);
    let assignments = &transition.state.episode().participants[1].assignments;
    assert_eq!(
        assignments.len(),
        2,
        "the old record is kept, a new one appended"
    );
    assert_eq!(assignments[0].completed_at, Some(Sequence(2)));
    assert_eq!(
        assignments[1].assigned_at,
        Sequence(2),
        "assigned at the completion that freed the seat, not the broadcast's origin",
    );
    assert!(matches!(
        transition.actions.as_slice(),
        [HostAction::DeliverHandoff { agent_id, handoff }]
            if agent_id == "two" && handoff.from == "one" && handoff.origin == Sequence(1)
    ));
    assert!(!transition.state.quiescent(), "two holds work again");
}

#[test]
fn a_completion_with_nothing_queued_hands_nothing_back() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let router = FirstRouter::default();
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let transition = apply(&driver, &state, "one", 1, complete(), &router).expect("completes");
    assert!(transition.actions.is_empty());
    assert_eq!(transition.state.episode().settled(), 1);
}

#[test]
fn an_ask_holds_the_askers_completion_until_the_asked_seat_commits_a_row() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let router = FirstRouter::default();
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let asked = apply(&driver, &state, "one", 1, ask("two"), &router)
        .expect("ask")
        .state;
    assert_eq!(
        asked
            .ledger()
            .awaiting("one")
            .map(|w| w.keys().cloned().collect::<Vec<_>>()),
        Some(vec!["two".into()]),
    );
    let refused = apply(&driver, &asked, "one", 2, complete(), &router);
    assert!(
        matches!(&refused, Err(Error::AwaitingReply { agent_id, waiting_on })
            if agent_id == "one" && waiting_on == &["two".to_string()]),
        "{refused:?}",
    );
    let answered = apply(&driver, &asked, "two", 3, post(), &router)
        .expect("answer")
        .state;
    assert!(
        answered.ledger().awaiting("one").is_none(),
        "any row from two is the answer"
    );
    apply(&driver, &answered, "one", 4, complete(), &router).expect("now it may finish");
}

#[test]
fn a_settled_seat_that_owes_an_answer_is_woken_for_it() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let router = FirstRouter::default();
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let two_done = apply(&driver, &state, "two", 1, complete(), &router)
        .expect("two")
        .state;
    assert_eq!(
        round_ids(&driver, &two_done),
        ["one"],
        "settled seats are not woken"
    );
    let asked = apply(&driver, &two_done, "one", 2, ask("two"), &router)
        .expect("ask")
        .state;
    assert_eq!(
        round_ids(&driver, &asked),
        ["one", "two"],
        "the seat asked is owed a turn to answer, after the pending ones",
    );
    let answered = apply(&driver, &asked, "two", 3, post(), &router)
        .expect("answer")
        .state;
    assert_eq!(round_ids(&driver, &answered), ["one"]);
}

#[test]
fn a_full_queue_returns_the_work_to_its_author() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 4)
        .expect("driver")
        .with_queue_depth(1)
        .expect("depth");
    let router = FirstRouter::default();
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let first = apply(&driver, &state, "one", 1, broadcast("a"), &router)
        .expect("first")
        .state;
    assert_eq!(first.ledger().queue_len("two"), 1);
    assert_eq!(
        first.seen().ran_for.get("one"),
        Some(&Sequence(0)),
        "one ran for what it holds"
    );
    let second = apply(&driver, &first, "one", 2, broadcast("b"), &router)
        .expect("second")
        .state;
    assert_eq!(
        second.ledger().queue_len("two"),
        1,
        "the full queue took nothing"
    );
    assert_eq!(
        second.seen().ran_for.get("one"),
        None,
        "nobody could take the work, so its author is owed another turn",
    );
    assert!(matches!(
        CompletionDriver::new(&hive, 4)
            .expect("driver")
            .with_queue_depth(0),
        Err(Error::ZeroQueueDepth)
    ));
}

#[test]
fn a_broadcast_budget_is_admitted_before_routing() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 4)
        .expect("driver")
        .with_broadcast_budget(Some(1));
    let router = FirstRouter::default();
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let spent = apply(&driver, &state, "one", 1, broadcast("a"), &router)
        .expect("first")
        .state;
    assert_eq!(spent.ledger().charged("one", Sequence(0)), 1);
    let refused = apply(&driver, &spent, "one", 2, broadcast("b"), &router);
    assert!(
        matches!(&refused, Err(Error::BudgetSpent { agent_id, assigned_at })
            if agent_id == "one" && *assigned_at == Sequence(0)),
        "{refused:?}",
    );
    assert_eq!(
        router.calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "a refusal costs no model call",
    );
}

#[test]
fn a_completion_is_refused_for_an_assignment_the_host_never_delivered() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let router = FirstRouter::default();
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let queued = apply(&driver, &state, "one", 1, broadcast("a"), &router)
        .expect("b")
        .state;
    let mut reassigned = apply(&driver, &queued, "two", 2, complete(), &router)
        .expect("drains")
        .state;
    assert_eq!(open_at(&reassigned, "two"), Some(Sequence(2)));
    reassigned.delivered("two", Sequence(1));
    let refused = apply(&driver, &reassigned, "two", 3, complete(), &router);
    assert!(
        matches!(&refused, Err(Error::UndeliveredAssignment { assigned_at, delivered_through, .. })
            if *assigned_at == Sequence(2) && *delivered_through == Sequence(1)),
        "{refused:?}",
    );
    reassigned.delivered("two", Sequence(2));
    apply(&driver, &reassigned, "two", 3, complete(), &router).expect("shown, so accepted");
}

#[test]
fn delivery_reports_do_not_invalidate_a_pending_round() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let router = FirstRouter::default();
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let round = driver.pending_round(&state).expect("round");
    let mut reported = state.clone();
    reported.delivered("one", Sequence(0));
    reported.turn_started("one");
    let route_policy = policy(4);
    let routing = BroadcastRouting {
        primary: Some(&router),
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let events = vec![committed("one", 1, post()), committed("two", 2, post())];
    run(driver.apply_committed_round(&reported, &round, events, Some(routing)))
        .expect("the round binds to what was committed, not to what was shown");
}

#[test]
fn seats_that_ran_and_were_shown_everything_are_stalled_not_woken() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let router = FirstRouter::default();
    let state = driver.start(episode(&["one", "two"])).expect("state");
    assert_eq!(
        round_ids(&driver, &state),
        ["one", "two"],
        "never ran: owed"
    );
    let ran = apply(&driver, &state, "one", 1, post(), &router)
        .expect("one")
        .state;
    let mut ran = apply(&driver, &ran, "two", 2, post(), &router)
        .expect("two")
        .state;
    assert_eq!(
        round_ids(&driver, &ran),
        ["one", "two"],
        "rows they have not been shown"
    );
    ran.delivered("one", Sequence(2));
    ran.delivered("two", Sequence(2));
    assert!(round_ids(&driver, &ran).is_empty());
    assert_eq!(ran.stalled(), ["one", "two"]);
    assert!(!ran.quiescent(), "stalled is not over");
}

#[test]
fn quiescence_is_complete_with_nothing_queued_and_nothing_awaited() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let router = FirstRouter::default();
    let state = driver.start(episode(&["one", "two"])).expect("state");
    assert!(!state.quiescent());
    let one = apply(&driver, &state, "one", 1, complete(), &router)
        .expect("one")
        .state;
    assert!(!one.quiescent());
    let both = apply(&driver, &one, "two", 2, complete(), &router)
        .expect("two")
        .state;
    assert!(both.quiescent());
    assert!(round_ids(&driver, &both).is_empty());
}

#[test]
fn a_state_written_before_the_ledger_still_loads() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let mut payload = serde_json::to_value(&state).expect("serializes");
    let object = payload.as_object_mut().expect("object");
    object.remove("ledger");
    object.remove("seen");
    let loaded: DriverState = serde_json::from_value(payload).expect("older wire form loads");
    assert_eq!(loaded, state);
    driver.resume(loaded).expect("and validates");
}
