//! What one accepted utterance becomes.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::support::{ASIDES, ask, commit, commit_with, dm, post, try_commit_with};
use crate::error::Error;
use crate::runtime::speech::Utterance;
use crate::{
    aside::{Audience, NoAsideReason},
    mention::MentionTarget,
};

fn agents(committed: &crate::runtime::speech::CommittedUtterance) -> Vec<String> {
    committed
        .mentions
        .iter()
        .filter_map(|mention| match &mention.target {
            MentionTarget::Agent { id } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_post_is_one_desk_row_with_its_mentions_resolved() {
    let committed = commit("solver", &post("@checker please verify, then @theory"));
    assert_eq!(committed.audience, Audience::Desk);
    assert_eq!(agents(&committed), vec!["checker", "theory"]);
    assert!(!committed.closing);
    assert_eq!(committed.refusal, None);
    assert_eq!(committed.content, "@checker please verify, then @theory");
}

#[test]
fn a_dm_takes_its_audience_from_the_field_and_not_from_the_prose() {
    let committed = commit("solver", &dm(&["checker"], "the depth measures 1.23n"));
    assert_eq!(
        committed.audience,
        Audience::Aside {
            members: vec!["checker".into()]
        },
        "the message names nobody, and the audience is still exactly who `to` named",
    );
    assert_eq!(agents(&committed), vec!["checker"]);
    assert_eq!(committed.refusal, None);
}

#[test]
fn a_dm_recipient_the_grammar_would_not_have_matched_still_reaches_them() {
    // The old host spelled `to` back into "@id" and re-read it through the
    // mention grammar. A body that opens a code span swallows the rest of the
    // line, so a recipient re-parsed out of prose could vanish silently.
    let committed = commit("solver", &dm(&["checker"], "`@checker` is a name in prose"));
    assert_eq!(
        committed.audience,
        Audience::Aside {
            members: vec!["checker".into()]
        },
        "the field is the address; masking applies to the body, not to `to`",
    );
    assert_eq!(agents(&committed), vec!["checker"]);
}

#[test]
fn a_dm_whose_text_names_a_peer_already_in_the_audience_hands_them_the_turn() {
    let committed = commit("solver", &dm(&["checker"], "@checker take this next"));
    assert_eq!(
        agents(&committed),
        vec!["checker"],
        "who reads it and who goes next are different questions, but nobody \
         outside the audience is ever one of the answers",
    );
    assert_eq!(
        committed.audience,
        Audience::Aside {
            members: vec!["checker".into()]
        },
    );
}

#[test]
fn a_dm_whose_text_names_a_peer_outside_the_audience_does_not_hand_them_the_content() {
    // `checker` is the only admitted reader. A body that also names `theory`
    // must not become a next-turn dispatch that carries this private content
    // to somebody who was never admitted to it — see the P1 fixed here.
    let committed = commit("solver", &dm(&["checker"], "@theory take this next"));
    assert!(
        agents(&committed).is_empty(),
        "a peer the `to` field never named does not receive the private body",
    );
    assert_eq!(
        committed.audience,
        Audience::Aside {
            members: vec!["checker".into()]
        },
    );
}

#[test]
fn a_refused_aside_is_not_posted_to_the_desk() {
    // Six rows is the budget; a seventh cannot be part of the same aside.
    let refused = try_commit_with(
        "solver",
        &dm(&["checker"], "one more"),
        ASIDES,
        6,
        false,
        false,
    );
    assert_eq!(
        refused,
        Err(Error::AsideRefused {
            reason: NoAsideReason::BudgetSpent
        }),
        "a refusal fails toward silence, never toward the room",
    );
}

#[test]
fn a_refused_aside_is_a_desk_row_only_when_the_host_opts_into_the_room() {
    let committed = try_commit_with(
        "solver",
        &dm(&["checker"], "one more"),
        ASIDES,
        6,
        false,
        true,
    )
    .expect("opted-in fallback commits");
    assert_eq!(committed.audience, Audience::Desk);
    assert_eq!(committed.refusal, Some(NoAsideReason::BudgetSpent));
    assert_eq!(committed.content, "one more");
}

#[test]
fn an_aside_the_policy_disables_is_refused_with_that_reason() {
    let off = crate::aside::AsidePolicy {
        enabled: false,
        ..ASIDES
    };
    let refused = try_commit_with("solver", &dm(&["checker"], "quietly"), off, 0, false, false);
    assert_eq!(
        refused,
        Err(Error::AsideRefused {
            reason: NoAsideReason::Disabled
        }),
        "a dm with asides off must not become a public desk row",
    );
    let marker = try_commit_with(
        "solver",
        &post("!aside @checker quietly"),
        off,
        0,
        false,
        false,
    );
    assert!(marker.is_err(), "nor does the marker spelling");
}

#[test]
fn a_dm_naming_more_peers_than_the_policy_allows_is_refused() {
    let refused = try_commit_with(
        "solver",
        &dm(&["checker", "theory"], "both of you"),
        ASIDES,
        0,
        false,
        false,
    );
    assert_eq!(
        refused,
        Err(Error::AsideRefused {
            reason: NoAsideReason::AudienceTooLarge
        })
    );
}

#[test]
fn the_marker_a_seat_writes_reaches_the_same_audience_as_the_tool() {
    let committed = commit("solver", &post("!aside @checker the depth measures 1.23n"));
    assert_eq!(
        committed.audience,
        Audience::Aside {
            members: vec!["checker".into()]
        },
        "a seat briefed on the grammar rather than the tool is not penalised",
    );
}

#[test]
fn completion_appends_its_row_and_says_the_agents_work_is_finished() {
    let committed = commit(
        "lead",
        &Utterance::CompleteEpisode {
            message: "Psi(10^18) = 62418970, signed off by @checker".into(),
        },
    );
    assert!(committed.closing, "the host is told, and the host decides");
    assert!(committed.completes_episode);
    assert!(!committed.broadcasting);
    assert_eq!(committed.audience, Audience::Desk);
    assert_eq!(
        committed.content, "Psi(10^18) = 62418970, signed off by @checker",
        "the message is never lost to the closing",
    );
}

#[test]
fn a_broadcast_appends_a_desk_row_and_requests_semantic_routing() {
    let committed = commit(
        "lead",
        &Utterance::Broadcast {
            message: "Have a solver derive the recurrence".into(),
        },
    );
    assert_eq!(committed.audience, Audience::Desk);
    assert!(committed.broadcasting);
    assert!(!committed.completes_episode);
    assert!(!committed.closing);
}

#[test]
fn a_post_that_names_nobody_hands_the_turn_to_nobody() {
    let committed = commit("solver", &post("still working"));
    assert!(committed.mentions.is_empty(), "{:?}", committed.mentions);
    assert_eq!(committed.audience, Audience::Desk);
    assert_eq!(committed.refusal, None);
}

#[test]
fn an_ask_is_private_to_the_seat_it_asks_and_records_whom() {
    let committed = commit("solver", &ask("checker", "is the depth bound tight?"));
    assert_eq!(
        committed.audience,
        Audience::Aside {
            members: vec!["checker".into()]
        },
        "a question is between the asker and the asked",
    );
    assert_eq!(agents(&committed), vec!["checker"]);
    assert_eq!(committed.asks, ["checker".to_string()]);
    assert_eq!(committed.refusal, None);
    assert!(!committed.completes_episode, "asking is not finishing");
    assert!(!committed.broadcasting);
    assert_eq!(committed.content, "is the depth bound tight?");
}

#[test]
fn a_post_asks_nobody() {
    let asks_found = commit("solver", &post("still working")).asks;
    assert!(asks_found.is_empty(), "{asks_found:?}");
    let asks_found = commit("solver", &dm(&["checker"], "fyi")).asks;
    assert!(asks_found.is_empty(), "{asks_found:?}");
}

#[test]
fn an_ask_the_policy_refuses_is_not_posted_unless_the_room_fallback_is_chosen() {
    let disabled = crate::aside::AsidePolicy {
        enabled: false,
        ..ASIDES
    };
    let ask_it = ask("checker", "is the depth bound tight?");
    assert!(try_commit_with("solver", &ask_it, disabled, 0, false, false).is_err());
    let committed = try_commit_with("solver", &ask_it, disabled, 0, false, true).expect("room");
    assert_eq!(committed.audience, Audience::Desk, "fails toward the room");
    assert!(committed.refusal.is_some(), "and says why");
    assert_eq!(
        committed.asks,
        ["checker".to_string()],
        "the obligation survives the refusal: the host still waits on checker",
    );
}

#[test]
fn addressed_peers_uses_dm_targets_without_repeating_or_addressing_the_author() {
    use super::support::{desks, members};
    use crate::runtime::speech::addressed_peers;
    use crate::{desk::DeskSet, roster::Roster};

    let members = members();
    let desks_value = desks();
    let roster = Roster::new(&members, &[], &[]);
    let desks = DeskSet::new(&desks_value, &[], &[], &[], &[]);
    let utterance = Utterance::Dm {
        to: vec![
            "checker".into(),
            "solver".into(),
            "checker".into(),
            "theory".into(),
        ],
        message: "private finding".into(),
    };
    assert_eq!(
        addressed_peers(&utterance, "solver", &roster, &desks),
        ["checker", "theory"]
    );
}

#[test]
fn addressed_peers_reads_text_mentions_in_authored_order() {
    use super::support::{desks, members};
    use crate::runtime::speech::addressed_peers;
    use crate::{desk::DeskSet, roster::Roster};

    let members = members();
    let desks_value = desks();
    let roster = Roster::new(&members, &[], &[]);
    let desks = DeskSet::new(&desks_value, &[], &[], &[], &[]);
    let utterance = post("@theory check with @checker; @theory will summarize");
    assert_eq!(
        addressed_peers(&utterance, "solver", &roster, &desks),
        ["theory", "checker"]
    );
}
