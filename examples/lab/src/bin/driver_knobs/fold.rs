//! The completion fold, hive validation, and the brief a seat is handed.

use tinyhivemind_core::desk::{Desk, ResponderMode};
use tinyhivemind_core::driver::{
    AgentBinding, BoundHive, Channel, CompletionDriver, ConversationView, ElsewhereView,
    EpisodeBrief, HiveGraph, speaker, standing_contract,
};
use tinyhivemind_core::hive::{
    CompletionEpisodeState, CompletionStep, apply_assignment, apply_completion, completion_status,
};
use tinyhivemind_core::runtime::Sequence;
use tinyhivemind_core::runtime::speech::tool_specs;
use tinyhivemind_lab::{Res, section};

use crate::fixture::{Runtime, SEATS, candidates, conversation, desk, episode};

fn show(step: &CompletionStep) -> String {
    match step {
        CompletionStep::Active { pending_ids } => format!("Active{pending_ids:?}"),
        CompletionStep::Complete { completed_ids } => {
            format!("Complete({} seats)", completed_ids.len())
        }
    }
}

fn err<T>(result: Result<T, tinyhivemind_core::hive::Error>) -> String {
    result.err().map_or("ok".into(), |e| e.to_string())
}

pub fn run() -> Res {
    section("hive::completion: who holds work, as a fold");
    let state = episode(1);
    println!(
        "  opened: {}; settled {}",
        show(&completion_status(&state)),
        state.settled()
    );
    let after = apply_completion(&state, "writer", Sequence(2))?;
    println!(
        "  writer completes at ^2: {}; settled {}",
        show(&completion_status(&after)),
        after.settled()
    );
    let again = apply_completion(&after, "writer", Sequence(2))?;
    println!("  redelivered ^2 is a no-op: {}", again == after);
    println!(
        "  completing with no open assignment: {}",
        err(apply_completion(&after, "writer", Sequence(3)))
    );
    println!(
        "  completing at or below the assignment: {}",
        err(apply_completion(&after, "coder", Sequence(1)))
    );
    println!(
        "  unknown participant: {}",
        err(apply_completion(&after, "ghost", Sequence(3)))
    );
    let reassigned = apply_assignment(&after, ["writer"], Sequence(4))?;
    println!(
        "  writer reassigned at ^4: {}; writer's open assignment {:?}",
        show(&completion_status(&reassigned)),
        reassigned
            .participants
            .iter()
            .find(|p| p.agent_id == "writer")
            .and_then(|p| p.open())
            .map(|r| r.assigned_at.0)
    );
    println!(
        "  assigning a seat that is still busy: {}",
        err(apply_assignment(&reassigned, ["writer"], Sequence(5)))
    );
    println!(
        "  assigning below the last assignment: {}",
        err(apply_assignment(&after, ["writer"], Sequence(1)))
    );
    println!(
        "  assigning nobody: {}",
        err(apply_assignment(&after, Vec::<String>::new(), Sequence(5)))
    );
    println!(
        "  assigning the same seat twice: {}",
        err(apply_assignment(&after, ["writer", "writer"], Sequence(5)))
    );
    println!(
        "  assigning an unknown seat: {}",
        err(apply_assignment(&after, ["ghost"], Sequence(5)))
    );
    println!(
        "  opening with no participants: {}",
        err(CompletionEpisodeState::opened(
            conversation(),
            Sequence(1),
            Vec::<String>::new()
        ))
    );
    println!(
        "  opening with a blank participant: {}",
        err(CompletionEpisodeState::opened(
            conversation(),
            Sequence(1),
            ["a", " "]
        ))
    );
    println!(
        "  opening with a duplicate: {}",
        err(CompletionEpisodeState::opened(
            conversation(),
            Sequence(1),
            ["a", "a"]
        ))
    );

    section("BoundHive: every way a hive is malformed");
    let bind = |ids: &[&str]| -> Vec<AgentBinding<Runtime>> {
        ids.iter()
            .map(|id| AgentBinding::new(*id, Runtime(format!("session-{id}"))))
            .collect()
    };
    let shown = |label: &str, graph: HiveGraph, bindings: Vec<AgentBinding<Runtime>>| {
        println!(
            "  {label:<30} {}",
            BoundHive::new(graph, bindings)
                .err()
                .map_or("valid".into(), |e| e.to_string())
        );
    };
    shown(
        "well formed",
        HiveGraph::new(desk(), candidates()),
        bind(&SEATS),
    );
    let mut blank = candidates();
    blank[0].id = " ".into();
    shown(
        "blank candidate id",
        HiveGraph::new(desk(), blank),
        bind(&SEATS),
    );
    let mut none = candidates();
    none[0].id = "none".into();
    shown(
        "candidate named none",
        HiveGraph::new(desk(), none),
        bind(&SEATS),
    );
    let mut twice = candidates();
    twice[1].id = twice[0].id.clone();
    shown(
        "duplicate candidate",
        HiveGraph::new(desk(), twice),
        bind(&SEATS),
    );
    shown(
        "missing binding",
        HiveGraph::new(desk(), candidates()),
        bind(&SEATS[..3]),
    );
    shown(
        "extra binding",
        HiveGraph::new(desk(), candidates()),
        bind(&["planner", "coder", "tester", "writer", "ghost"]),
    );
    shown(
        "duplicate binding",
        HiveGraph::new(desk(), candidates()),
        bind(&["planner", "planner", "tester", "writer"]),
    );
    let empty = Desk {
        members: vec![],
        ..desk()
    };
    shown(
        "a desk with no members",
        HiveGraph::new(empty, vec![]),
        vec![],
    );
    let dup_member = Desk {
        members: vec!["planner".into(), "planner".into()],
        ..desk()
    };
    shown(
        "duplicate desk member",
        HiveGraph::new(dup_member, candidates()),
        bind(&SEATS),
    );
    let blank_desk = Desk {
        id: String::new(),
        ..desk()
    };
    shown(
        "invalid desk",
        HiveGraph::new(blank_desk, candidates()),
        bind(&SEATS),
    );
    let hive = BoundHive::new(
        HiveGraph::new(
            Desk {
                responder_mode: ResponderMode::Auto,
                ..desk()
            },
            candidates(),
        ),
        bind(&SEATS),
    )?;
    println!(
        "  members {:?}; binding(coder) runtime {:?}",
        hive.members().collect::<Vec<_>>(),
        hive.binding("coder").map(|b| b.runtime_agent_id())
    );
    println!(
        "  resolve_dm too wide: {}",
        hive.resolve_dm("planner", &["coder".into(), "tester".into()], 1)
            .err()
            .map_or("ok".into(), |e| e.to_string())
    );
    println!(
        "  resolve_dm unknown sender: {}",
        hive.resolve_dm("ghost", &["coder".into()], 2)
            .err()
            .map_or("ok".into(), |e| e.to_string())
    );
    println!(
        "  resolve_dm duplicate recipient: {}",
        hive.resolve_dm("planner", &["coder".into(), "coder".into()], 2)
            .err()
            .map_or("ok".into(), |e| e.to_string())
    );
    println!(
        "  resolve_dm nobody: {}",
        hive.resolve_dm("planner", &[], 2)
            .err()
            .map_or("ok".into(), |e| e.to_string())
    );
    println!(
        "  driver desk is {:?}; a driver over it starts at {:?}",
        hive.desk().id,
        CompletionDriver::new(&hive, 1)?
            .start(episode(1))?
            .revision()
    );

    section("EpisodeBrief: what a seat is handed before its turn");
    let driver = CompletionDriver::new(&hive, 2)?;
    let state = driver.start(episode(1))?;
    let mut brief = EpisodeBrief::for_turn(
        &state,
        "eng",
        "coder",
        Channel::Desk,
        vec!["[2] @planner: implement the rust parser".into()],
        vec![ConversationView {
            root: Sequence(3),
            others: vec!["tester".into()],
            opened_it: true,
            transcript: vec![
                "[3] @coder: which edge cases?".into(),
                "[4] @tester: empty input".into(),
            ],
            concluded: true,
        }],
    );
    brief.elsewhere.push(ElsewhereView {
        chat: "ops".into(),
        name: "Operations".into(),
        thread_root: None,
        rows: vec!["@dave: pager is quiet".into()],
    });
    brief.name_seats(|id| id.to_uppercase());
    println!(
        "  speaker(tester) = {:?}; free speaker(\"x\", \"x\") = {:?}; parent() = {:?}",
        brief.speaker("tester"),
        speaker("x", "x"),
        brief.parent()
    );
    let rendered = brief.render();
    for line in rendered.lines().take(14) {
        println!("    | {line}");
    }
    println!(
        "    | ... ({} chars, about {} tokens at 4 chars each)",
        rendered.len(),
        rendered.len() / 4
    );
    let thread = EpisodeBrief::for_turn(
        &state,
        "eng",
        "tester",
        Channel::Thread {
            root: Sequence(3),
            others: vec!["coder".into()],
            opened_it: false,
        },
        Vec::new(),
        Vec::new(),
    );
    println!(
        "  a thread brief for the askee is {} chars; parent() = {:?}",
        thread.render().len(),
        thread.parent()
    );
    let seats: Vec<String> = SEATS.iter().map(|s| (*s).to_owned()).collect();
    let contract = standing_contract(
        tool_specs(),
        "eng",
        &seats,
        "Call them through the desk server.",
    );
    println!(
        "  standing_contract is {} chars (about {} tokens), resent on every model call",
        contract.len(),
        contract.len() / 4
    );
    Ok(())
}
