//! Continuous sharing: deltas, the watermark, and why a seat must re-seed.

use tinyhivemind_core::aside::{Audience, Viewer};
use tinyhivemind_core::runtime::sharing::{
    PRESENT_SET_LIMIT, SharingPlan, SharingQuery, SharingState, initialized_state, note_present,
    prepare_delta,
};
use tinyhivemind_core::runtime::{Conversation, Sequence};
use tinyhivemind_lab::{MemoryLog, Res, agent, block_on, section};

use crate::eng;

pub fn sharing() -> Res {
    section("sharing: prepare_delta, watermark, ReinitializeReason");
    let mut log = MemoryLog::default();
    for n in 1..=5 {
        log.say("eng", agent("alice"), &format!("history {n}"));
    }
    log.say("eng", agent("alice"), "visible new row");
    log.append(
        "eng",
        None,
        agent("alice"),
        "whisper to bob",
        Audience::Aside {
            members: vec!["bob".into()],
        },
    );
    log.say("eng", agent("bob"), "another new row");
    let conv = eng();
    let state = initialized_state(conv.clone(), Sequence(5));
    for (who, id) in [("bob", "bob"), ("carol", "carol")] {
        let viewer = Viewer::Agent { id: id.into() };
        let plan = block_on(prepare_delta(
            &log,
            &SharingQuery {
                desired_conversation: &conv,
                current_conversation: &conv,
                state: &state,
                viewer: &viewer,
                before: Sequence(9),
            },
        ))?;
        if let SharingPlan::Delta(delta) = plan {
            let rows: Vec<String> = delta
                .messages
                .iter()
                .map(|m| {
                    format!(
                        "^{}:{}",
                        m.sequence,
                        if m.elided.is_some() {
                            "<elided>"
                        } else {
                            "text"
                        }
                    )
                })
                .collect();
            println!(
                "  {who}: delta {rows:?} next watermark ^{}",
                delta.next_state.watermark
            );
        }
    }
    let mut seeded = state.clone();
    note_present(&mut seeded, Sequence(6))?;
    let viewer = Viewer::Agent { id: "bob".into() };
    let plan = block_on(prepare_delta(
        &log,
        &SharingQuery {
            desired_conversation: &conv,
            current_conversation: &conv,
            state: &seeded,
            viewer: &viewer,
            before: Sequence(9),
        },
    ))?;
    if let SharingPlan::Delta(delta) = plan {
        println!(
            "  after note_present(^6): delta carries {:?}",
            delta
                .messages
                .iter()
                .map(|m| m.sequence.0)
                .collect::<Vec<_>>()
        );
    }
    let other = Conversation {
        thread_root: Some(Sequence(2)),
        ..conv.clone()
    };
    let changed = block_on(prepare_delta(
        &log,
        &SharingQuery {
            desired_conversation: &other,
            current_conversation: &conv,
            state: &state,
            viewer: &viewer,
            before: Sequence(9),
        },
    ))?;
    println!("  conversation changed -> {changed:?}");

    let mut deep = MemoryLog::default();
    for n in 1..=2100 {
        deep.say("eng", agent("alice"), &format!("row {n}"));
    }
    let far = initialized_state(conv.clone(), Sequence(1));
    let gap = block_on(prepare_delta(
        &deep,
        &SharingQuery {
            desired_conversation: &conv,
            current_conversation: &conv,
            state: &far,
            viewer: &viewer,
            before: Sequence(2101),
        },
    ))?;
    println!("  watermark beyond the scan -> {gap:?}");

    let mut compacted = MemoryLog::starting_after(49);
    compacted.say("eng", agent("alice"), "oldest surviving row");
    compacted.say("eng", agent("alice"), "newer");
    let behind = initialized_state(conv.clone(), Sequence(20));
    let missing = block_on(prepare_delta(
        &compacted,
        &SharingQuery {
            desired_conversation: &conv,
            current_conversation: &conv,
            state: &behind,
            viewer: &viewer,
            before: Sequence(52),
        },
    ))?;
    println!("  log compacted past the watermark -> {missing:?}");

    let regress = block_on(prepare_delta(
        &log,
        &SharingQuery {
            desired_conversation: &conv,
            current_conversation: &conv,
            state: &state,
            viewer: &viewer,
            before: Sequence(3),
        },
    ));
    println!(
        "  before below watermark -> error: {}",
        regress.err().map_or("none".into(), |e| e.to_string())
    );
    let mut full = initialized_state(conv.clone(), Sequence(0));
    let mut overflow = None;
    for n in 1..=(PRESENT_SET_LIMIT as u64 + 1) {
        if let Err(error) = note_present(&mut full, Sequence(n)) {
            overflow = Some(error.to_string());
        }
    }
    println!(
        "  note_present past PRESENT_SET_LIMIT={PRESENT_SET_LIMIT} -> {}",
        overflow.unwrap_or_default()
    );
    let wire = format!(
        "{{\"conversation\":{},\"watermark\":0,\"present_above_watermark\":[{}]}}",
        serde_json::to_string(&conv)?,
        (1..=PRESENT_SET_LIMIT + 1)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    println!(
        "  decoding an oversized SharingState: {}",
        serde_json::from_str::<SharingState>(&wire)
            .err()
            .map_or("accepted".into(), |e| e.to_string())
    );
    Ok(())
}
