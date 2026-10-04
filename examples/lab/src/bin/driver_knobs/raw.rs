//! The completion driver on its own: width, queue depth, budget, replay.

use tinyhivemind_core::driver::{
    BroadcastRouting, CommittedUtterance, CompletionDriver, DriverState, HostAction,
};
use tinyhivemind_core::runtime::Sequence;
use tinyhivemind_core::runtime::speech::Utterance;
use tinyhivemind_lab::{KeywordRouter, Res, block_on, section};

use crate::fixture::{Runtime, conversation, episode, hive, routing_policy};

fn say(author: &str, sequence: u64, utterance: Utterance) -> CommittedUtterance {
    CommittedUtterance {
        author_id: author.into(),
        sequence: Sequence(sequence),
        utterance,
    }
}

fn complete(author: &str, sequence: u64) -> CommittedUtterance {
    say(
        author,
        sequence,
        Utterance::CompleteEpisode {
            message: "done".into(),
        },
    )
}

fn broadcast(author: &str, sequence: u64, text: &str) -> CommittedUtterance {
    say(
        author,
        sequence,
        Utterance::Broadcast {
            message: text.into(),
        },
    )
}

fn actions(actions: &[HostAction]) -> String {
    if actions.is_empty() {
        return "no actions".into();
    }
    actions
        .iter()
        .map(|action| match action {
            HostAction::RunAgents { agent_ids, .. } => format!("RunAgents{agent_ids:?}"),
            HostAction::DeliverDm { route, .. } => format!("DeliverDm({route:?})"),
            HostAction::DeliverHandoff { agent_id, handoff } => {
                format!("DeliverHandoff(to {agent_id} from {})", handoff.from)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Apply events in order, printing what each did to the state.
fn drive(
    label: &str,
    driver: &CompletionDriver<'_, Runtime>,
    routing: Option<BroadcastRouting<'_>>,
    events: Vec<CommittedUtterance>,
) -> Option<DriverState> {
    let mut state = driver.start(episode(1)).ok()?;
    println!("  {label}");
    for event in events {
        let (who, at) = (event.author_id.clone(), event.sequence.0);
        match block_on(driver.apply_committed(&state, event, routing)) {
            Ok(transition) => {
                println!(
                    "    ^{at} {who:<8} -> {}; queued for coder: {}",
                    actions(&transition.actions),
                    transition.state.ledger().queue_len("coder")
                );
                state = transition.state;
            }
            Err(error) => println!("    ^{at} {who:<8} -> REFUSED: {error}"),
        }
    }
    Some(state)
}

pub fn run() -> Res {
    section("CompletionDriver: construction, width and pending rounds");
    let hive = hive();
    println!(
        "  round_width=0: {}",
        CompletionDriver::new(&hive, 0)
            .err()
            .map_or("ok".into(), |e| e.to_string())
    );
    let probe = CompletionDriver::new(&hive, 2)?;
    println!(
        "  queue_depth=0: {}",
        probe
            .with_queue_depth(0)
            .err()
            .map_or("ok".into(), |e| e.to_string())
    );
    for width in [1, 2, 3, 4] {
        let driver = CompletionDriver::new(&hive, width)?;
        let state = driver.start(episode(1))?;
        let round = driver.pending_round(&state)?;
        let ids: Vec<&str> = round.agents().iter().map(|a| a.hive_agent_id).collect();
        let runtimes: Vec<&str> = round.agents().iter().map(|a| a.agent.0.as_str()).collect();
        println!(
            "  round_width={width}: pending_round of {} = {ids:?} (runtimes {runtimes:?}); is_empty={}",
            ids.len(),
            round.is_empty()
        );
    }

    section("broadcast: routing, queue_depth and the budget");
    let router = KeywordRouter::new("scripted");
    let narrow = routing_policy(1);
    let wide = routing_policy(3);
    let routing = |policy| BroadcastRouting {
        primary: Some(&router),
        reasoning: None,
        policy,
        roster_version: 1,
        thread_context: &[],
    };
    // tester and writer finish; coder and planner stay busy.
    let setup = |extra: Vec<CommittedUtterance>| {
        let mut events = vec![complete("tester", 2), complete("writer", 3)];
        events.extend(extra);
        events
    };
    for depth in [1, 2] {
        let driver = CompletionDriver::new(&hive, 2)?.with_queue_depth(depth)?;
        drive(
            &format!("queue_depth={depth}: planner then tester hand work to the busy coder"),
            &driver,
            Some(routing(&narrow)),
            setup(vec![
                broadcast("planner", 4, "write the rust parser"),
                broadcast("tester", 5, "also the rust parser edge cases"),
                broadcast("writer", 6, "and the rust parser docs"),
            ]),
        );
    }
    let driver = CompletionDriver::new(&hive, 1)?;
    drive(
        "driver round_width=1 against a router that invites two seats (policy width 3)",
        &driver,
        Some(routing(&wide)),
        setup(vec![broadcast(
            "planner",
            4,
            "write the rust parser and its tests",
        )]),
    );
    let driver = CompletionDriver::new(&hive, 3)?;
    drive(
        "driver round_width=3 against the same router",
        &driver,
        Some(routing(&wide)),
        setup(vec![broadcast(
            "planner",
            4,
            "write the rust parser and its tests",
        )]),
    );
    drive(
        "a broadcast with no routing supplied",
        &driver,
        None,
        setup(vec![broadcast("planner", 4, "anything")]),
    );
    for budget in [None, Some(1), Some(2)] {
        let driver = CompletionDriver::new(&hive, 2)?.with_broadcast_budget(budget);
        drive(
            &format!("broadcast_budget={budget:?}: coder broadcasts three times in one assignment"),
            &driver,
            Some(routing(&narrow)),
            setup(vec![
                broadcast("coder", 4, "plan the roadmap"),
                broadcast("coder", 5, "plan the roadmap again"),
                broadcast("coder", 6, "plan the roadmap once more"),
            ]),
        );
    }
    println!("  router asked {} times across these runs", router.calls());

    section("asks, DMs, and what the ledger refuses");
    let driver = CompletionDriver::new(&hive, 2)?;
    let ask = |to: &[&str]| Utterance::Ask {
        to: to.iter().map(|s| (*s).to_owned()).collect(),
        message: "which cases?".into(),
    };
    let state = drive(
        "coder asks tester; planner asks three seats at width 2; coder tries to finish with the ask open",
        &driver,
        Some(routing(&narrow)),
        vec![
            say("coder", 2, ask(&["tester"])),
            say(
                "planner",
                3,
                Utterance::Dm {
                    to: vec!["coder".into(), "tester".into(), "writer".into()],
                    message: "sync".into(),
                },
            ),
            say(
                "planner",
                4,
                Utterance::Dm {
                    to: vec!["ghost".into()],
                    message: "who?".into(),
                },
            ),
            say(
                "coder",
                5,
                Utterance::CompleteEpisode {
                    message: "done early".into(),
                },
            ),
            say(
                "tester",
                6,
                Utterance::Dm {
                    to: vec!["coder".into()],
                    message: "empty input; nested quotes".into(),
                },
            ),
            say(
                "coder",
                7,
                Utterance::CompleteEpisode {
                    message: "done now".into(),
                },
            ),
        ],
    );
    if let Some(state) = state {
        println!(
            "    ledger outstanding asks: {:?}; quiescent={}; stalled={:?}",
            state.ledger().outstanding_asks,
            state.quiescent(),
            state.stalled()
        );
    }

    section("replay, ordering, snapshot and revision");
    let driver = CompletionDriver::new(&hive, 2)?;
    let state = driver.start(episode(1))?;
    let first = block_on(driver.apply_committed(&state, complete("writer", 2), None))?;
    let again = block_on(driver.apply_committed(&first.state, complete("writer", 2), None))?;
    println!(
        "  redelivering ^2 changes nothing: {} (revision {} -> {})",
        again.state == first.state,
        first.state.revision(),
        again.state.revision()
    );
    let clash = block_on(driver.apply_committed(&first.state, complete("tester", 2), None));
    println!(
        "  another event at ^2: {}",
        clash.err().map_or("ok".into(), |e| e.to_string())
    );
    let stale = block_on(driver.apply_committed(&first.state, complete("tester", 1), None));
    println!(
        "  an event below the freshness floor: {}",
        stale.err().map_or("ok".into(), |e| e.to_string())
    );
    let stranger = block_on(driver.apply_committed(&first.state, complete("ghost", 3), None));
    println!(
        "  an author the hive does not bind: {}",
        stranger.err().map_or("ok".into(), |e| e.to_string())
    );
    let json = serde_json::to_string(&first.state)?;
    let revived: DriverState = serde_json::from_str(&json)?;
    println!(
        "  JSON round trip is {} bytes; resume accepts it: {}",
        json.len(),
        driver.resume(revived).is_ok()
    );
    let tampered = json.replacen("\"revision\":1", "\"revision\":7", 1);
    let bad: DriverState = serde_json::from_str(&tampered)?;
    println!(
        "  a tampered revision: {}",
        driver
            .resume(bad)
            .err()
            .map_or("ok".into(), |e| e.to_string())
    );
    let mut shown = first.state.clone();
    shown.delivered("coder", Sequence(0));
    let undelivered = block_on(driver.apply_committed(&shown, complete("coder", 3), None));
    println!(
        "  completing an assignment the seat was never shown: {}",
        undelivered.err().map_or("ok".into(), |e| e.to_string())
    );
    shown.delivered("coder", Sequence(1));
    shown.turn_started("coder");
    println!(
        "  after delivered+turn_started the host-side ledger says seen = {:?}",
        shown.seen()
    );
    let other = CompletionDriver::new(&hive, 2)?;
    println!("  episode conversation desk: {}", conversation().desk_id);
    let _ = other;
    Ok(())
}
