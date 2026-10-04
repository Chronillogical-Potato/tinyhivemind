//! One desk's worth of utterances, played through the public surface.
//!
//! This is the regression that pins the run-28 path: a fixed script of tool
//! calls, and the exact rows, audiences, mentions and closing decision the room
//! makes of them. It drives only `tinyhivemind_core::runtime::speech`, so it fails if the
//! meaning of an utterance moves — which is the whole risk of having moved it
//! out of a host in the first place.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use tinyhivemind_core::runtime::{
    aside::{AsidePolicy, Audience, NoAsideReason},
    desk::{Desk, DeskSet, ResponderMode},
    dispatch::DispatchConversation,
    mention::MentionTarget,
    roster::{Person, Roster, RosterMember},
    speech::{
        CallArguments, CommitRequest, CommittedUtterance, ToolCall, commit_utterance,
        commit_utterance_to_room, interpret,
    },
};

/// The `desk` example's policy: one peer, six rows, a settlement owed.
const ASIDES: AsidePolicy = AsidePolicy {
    enabled: true,
    max_members: 1,
    max_messages: 6,
    must_surface: true,
    require_thread: false,
};

/// One appended row, as much of it as this test cares about.
#[derive(Debug, Eq, PartialEq)]
struct Row {
    content: String,
    audience: Audience,
    routed_to: Vec<String>,
    closing: bool,
    refusal: Option<NoAsideReason>,
}

/// Play a script of `(seat, tool, message, to)` calls through the room.
///
/// The aside bookkeeping is folded from the rows already produced, exactly as
/// a host folds it from its journal: the run of consecutive private rows at the
/// tail is what the open aside has spent.
fn play(script: &[(&str, &str, &str, &[&str])]) -> Vec<Row> {
    play_as(script, false).expect("no aside in the script is refused")
}

/// [`play`], returning the first refusal, with the room fallback opted into
/// when `room` is set.
fn play_as(
    script: &[(&str, &str, &str, &[&str])],
    room: bool,
) -> Result<Vec<Row>, tinyhivemind_core::error::Error> {
    let members: Vec<RosterMember> = ["lead", "solver", "theory", "checker"]
        .into_iter()
        .map(|id| RosterMember {
            id: id.into(),
            name: Some(id.to_uppercase()),
        })
        .collect();
    let people = vec![Person {
        id: "steven".into(),
        label: "Steven".into(),
    }];
    let declared = [Desk {
        id: "pe1006".into(),
        name: "PE 1006".into(),
        description: None,
        members: members.iter().map(|member| member.id.clone()).collect(),
        responder_mode: ResponderMode::Lead,
    }];
    let roster = Roster::new(&members, &people, &[]);
    let desks = DeskSet::new(&declared, &[], &[], &[], &[]);
    let conversation = DispatchConversation {
        desk_id: "pe1006".into(),
        thread_root: None,
    };

    let mut rows: Vec<Row> = Vec::new();
    for (seat, tool, message, to) in script {
        let recipients: Vec<String> = to.iter().map(|id| (*id).to_string()).collect();
        let call = interpret(
            tool,
            &CallArguments {
                message: Some(message),
                to: &recipients,
                limit: None,
            },
        )
        .expect("every call in the script is well formed");
        let ToolCall::Speak(utterance) = call else {
            panic!("the script speaks; it does not read");
        };
        let spent = rows
            .iter()
            .rev()
            .take_while(|row| !matches!(row.audience, Audience::Desk))
            .count();
        let request = CommitRequest {
            utterance: &utterance,
            speaker_id: seat,
            conversation: &conversation,
            aside: ASIDES,
            spent,
            unsettled: false,
            roster: &roster,
            desks: &desks,
        };
        let committed = if room {
            commit_utterance_to_room(&request)?
        } else {
            commit_utterance(&request)?
        };
        rows.push(row(&committed));
        if committed.closing {
            break;
        }
    }
    Ok(rows)
}

fn row(committed: &CommittedUtterance) -> Row {
    Row {
        content: committed.content.clone(),
        audience: committed.audience.clone(),
        routed_to: committed
            .mentions
            .iter()
            .filter(|mention| !mention.quiet)
            .filter_map(|mention| match &mention.target {
                MentionTarget::Agent { id } => Some(id.clone()),
                _ => None,
            })
            .collect(),
        closing: committed.closing,
        refusal: committed.refusal,
    }
}

fn desk(content: &str, routed_to: &[&str]) -> Row {
    Row {
        content: content.into(),
        audience: Audience::Desk,
        routed_to: routed_to.iter().map(|id| (*id).to_string()).collect(),
        closing: false,
        refusal: None,
    }
}

#[test]
fn a_desk_that_works_a_problem_and_closes_it_commits_exactly_these_rows() {
    let played = play(&[
        (
            "theory",
            "post",
            "@solver B(x,n) is the factor count; here is the recursion",
            &[],
        ),
        (
            "solver",
            "post",
            "ran factors.py: Psi(3)=20302. @checker please verify",
            &[],
        ),
        (
            "checker",
            "dm",
            "your depth measures 1.23n, not log n",
            &["solver"],
        ),
        (
            "solver",
            "post",
            "@checker you are right; closed the Cross(n,k) bug",
            &[],
        ),
        (
            "checker",
            "post",
            "SIGNOFF: 62418970 — recomputed independently",
            &[],
        ),
        (
            "lead",
            "complete_episode",
            "ANSWER: 62418970. @checker signed off; nothing is open",
            &[],
        ),
        ("lead", "post", "this row is never reached", &[]),
    ]);

    assert_eq!(
        played,
        vec![
            desk(
                "@solver B(x,n) is the factor count; here is the recursion",
                &["solver"],
            ),
            desk(
                "ran factors.py: Psi(3)=20302. @checker please verify",
                &["checker"],
            ),
            Row {
                content: "your depth measures 1.23n, not log n".into(),
                audience: Audience::Aside {
                    members: vec!["solver".into()],
                },
                routed_to: vec!["solver".into()],
                closing: false,
                refusal: None,
            },
            desk(
                "@checker you are right; closed the Cross(n,k) bug",
                &["checker"],
            ),
            desk("SIGNOFF: 62418970 — recomputed independently", &[]),
            Row {
                content: "ANSWER: 62418970. @checker signed off; nothing is open".into(),
                audience: Audience::Desk,
                routed_to: vec!["checker".into()],
                closing: true,
                refusal: None,
            },
        ],
        "explicit completion ends the desk holding its message, and nothing after it runs",
    );
}

#[test]
fn an_aside_that_outruns_its_budget_is_refused_not_posted() {
    let mut script: Vec<(&str, &str, &str, &[&str])> = Vec::new();
    for _ in 0..6 {
        script.push(("solver", "dm", "still checking", &["checker"]));
    }
    script.push(("solver", "dm", "one more", &["checker"]));
    assert_eq!(
        play_as(&script, false),
        Err(tinyhivemind_core::error::Error::AsideRefused {
            reason: NoAsideReason::BudgetSpent
        }),
        "the seventh private message is never a public row",
    );
}

#[test]
fn an_aside_that_outruns_its_budget_falls_back_to_the_room_when_the_host_opts_in() {
    let mut script: Vec<(&str, &str, &str, &[&str])> = Vec::new();
    for _ in 0..6 {
        script.push(("solver", "dm", "still checking", &["checker"]));
    }
    script.push(("solver", "dm", "one more", &["checker"]));
    let played = play_as(&script, true).expect("room fallback commits");

    assert_eq!(played.len(), 7);
    for row in &played[..6] {
        assert_eq!(
            row.audience,
            Audience::Aside {
                members: vec!["checker".into()]
            },
        );
        assert_eq!(row.refusal, None);
    }
    assert_eq!(
        played[6],
        Row {
            content: "one more".into(),
            audience: Audience::Desk,
            routed_to: vec!["checker".into()],
            closing: false,
            refusal: Some(NoAsideReason::BudgetSpent),
        },
        "the seventh is said in the open, with the reason it went there",
    );
}
