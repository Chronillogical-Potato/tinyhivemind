//! Cross-inhibition: the mechanism that silences an advocate rather than
//! debiting an option, and the proof that it is load-bearing — it can break a
//! tie that a subtracted score never could.

use super::super::*;
use super::support::{deadlocked_transcript, fold, policy, said, standing};

#[test]
fn two_equally_supported_topics_deadlock() {
    let standings = fold(&deadlocked_transcript(), &policy(2));
    assert_eq!(
        consensus(&standings, &policy(2)),
        ConsensusState::Deadlocked {
            topics: vec!["stage".into(), "ship".into()],
        },
    );
}

#[test]
fn a_grounded_objection_silences_an_advocate_and_breaks_the_deadlock() {
    let mut transcript = deadlocked_transcript();
    // The objection names archivist's supporting message, not the topic.
    transcript.push(said(
        5,
        "critic",
        "!object >4 ^3 That precedent was a different system.",
    ));

    let standings = fold(&transcript, &policy(2));
    let ship = standing(&standings, "ship");
    assert_eq!(ship.silenced, ["archivist"]);
    assert_eq!(ship.supporters, ["scout"]);
    assert_eq!(
        consensus(&standings, &policy(2)),
        ConsensusState::Quorum {
            topic: "stage".into()
        },
        "silencing one advocate must break the tie the same input deadlocks on",
    );
}

#[test]
fn silencing_the_last_advocate_zeroes_a_topics_weight() {
    let transcript = [
        said(1, "planner", "!propose #stage"),
        said(2, "critic", "!object >1 ^0 Not this."),
    ];
    let standings = fold(&transcript, &policy(2));
    let held = standing(&standings, "stage");
    assert!(held.supporters.is_empty(), "{:?}", held.supporters);
    assert_eq!(held.silenced, ["planner"]);
    assert_eq!(held.support, 0);
}

#[test]
fn an_objection_cannot_silence_its_own_author() {
    // Otherwise an agent could retract a peer's support by objecting to itself.
    let transcript = [
        said(1, "planner", "!propose #stage"),
        said(2, "planner", "!object >1 ^1 Second thoughts."),
    ];
    assert_eq!(
        standing(&fold(&transcript, &policy(2)), "stage").supporters,
        ["planner"]
    );
}

#[test]
fn an_ungrounded_objection_silences_nobody() {
    let transcript = [
        said(1, "planner", "!propose #stage"),
        said(2, "critic", "!object >1"),
    ];
    let standings = fold(&transcript, &policy(2));
    let silenced = &standing(&standings, "stage").silenced;
    assert!(silenced.is_empty(), "{silenced:?}");
}

#[test]
fn an_objection_at_an_unknown_or_absent_target_silences_nobody() {
    let transcript = [
        said(1, "planner", "!propose #stage"),
        said(2, "critic", "!object >99 ^1"),
        said(3, "critic", "!object ^1"),
    ];
    let standings = fold(&transcript, &policy(2));
    let silenced = &standing(&standings, "stage").silenced;
    assert!(silenced.is_empty(), "{silenced:?}");
}

#[test]
fn an_objection_silences_every_topic_a_targeted_message_advocated() {
    // planner's single message advocates two topics at two offsets. An
    // objection naming that message must silence planner as the advocate of
    // *both* topics, not just whichever one a sequence-keyed map happened to
    // remember last.
    let transcript = vec![
        said(1, "planner", "!propose #stage\n!propose #ship"),
        said(2, "critic", "!support #stage ^1 Bounds the blast radius."),
        said(3, "archivist", "!support #ship ^1 We shipped this before."),
        said(4, "auditor", "!object >1 ^1 Neither precedent holds."),
    ];
    let standings = fold(&transcript, &policy(2));

    let stage = standing(&standings, "stage");
    assert!(
        !stage.supporters.contains(&"planner".to_owned()),
        "planner advocated #stage in the targeted message and must be silenced there too",
    );
    assert!(stage.silenced.contains(&"planner".to_owned()));

    let ship = standing(&standings, "ship");
    assert!(!ship.supporters.contains(&"planner".to_owned()));
    assert!(ship.silenced.contains(&"planner".to_owned()));
}

#[test]
fn silencing_one_of_two_advocates_leaves_only_the_survivors_weight() {
    // planner proposes (900) and critic supports (500). Objecting to planner
    // alone must drop planner's 900 from the topic's weight entirely, not
    // just leave the topic non-empty with both contributions still summed.
    let transcript = vec![
        said(1, "planner", "!propose #stage Stage the rollout."),
        said(2, "critic", "!support #stage ^1 Bounds the blast radius."),
        said(3, "auditor", "!object >1 ^1 Reconsider the precedent."),
    ];
    let standings = fold(&transcript, &policy(2));
    let stage = standing(&standings, "stage");

    assert_eq!(
        stage.supporters,
        ["critic"],
        "planner is silenced, critic remains",
    );
    assert_eq!(
        stage.support, 500,
        "the surviving weight must be critic's contribution alone, not \
         planner's silenced 900 plus critic's 500",
    );
}

#[test]
fn a_repeated_objection_records_an_advocate_once() {
    let transcript = [
        said(1, "planner", "!propose #stage"),
        said(2, "critic", "!object >1 ^0"),
        said(3, "scout", "!object >1 ^0"),
    ];
    assert_eq!(
        standing(&fold(&transcript, &policy(2)), "stage").silenced,
        ["planner"]
    );
}
