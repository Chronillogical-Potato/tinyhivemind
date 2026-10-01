//! Private-aside visibility tests: what each viewer reads, how a run of
//! elided rows collapses into one stub, and how a stub settles.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use super::support::{FakeLog, conversation, page, query};
use tinyhivemind_core::aside::Audience;
use tinyhivemind_core::aside::Viewer;

// ---------------------------------------------------------------------------
// Private asides
// ---------------------------------------------------------------------------

/// One desk-visible row authored by `id`.
fn said(sequence: u64, id: &str, content: &str) -> LogMessage {
    LogMessage {
        sequence: Sequence(sequence),
        chat_id: Some("engineering".into()),
        parent: None,
        author: SessionAuthor::Agent {
            id: id.into(),
            label: id.into(),
        },
        content: content.into(),
        audience: Audience::Desk,
    }
}

/// One row `id` addressed privately to `members`.
fn aside_row(sequence: u64, id: &str, members: &[&str], content: &str) -> LogMessage {
    LogMessage {
        audience: Audience::Aside {
            members: members.iter().map(|member| (*member).to_owned()).collect(),
        },
        ..said(sequence, id, content)
    }
}

fn agent(id: &str) -> Viewer {
    Viewer::Agent { id: id.into() }
}

fn as_viewer(rows: Vec<LogMessage>, viewer: Viewer) -> Vec<SessionMessage> {
    let log = FakeLog::new(vec![page(rows.into_iter().rev().collect::<Vec<_>>(), None)]);
    let query = SessionQuery {
        viewer,
        ..query(30)
    };
    futures_lite_block_on(project_session(&log, &query))
}

/// Drive one future to completion on this thread.
///
/// The crate is executor-neutral and its port returns a boxed future, so a
/// unit test needs some way to poll one. `tokio` is already the dev-dependency
/// everything else here uses.
fn futures_lite_block_on(
    future: impl std::future::Future<Output = Result<Vec<SessionMessage>>>,
) -> Vec<SessionMessage> {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a current-thread runtime")
        .block_on(future)
        .expect("projects")
}

fn desk_transcript() -> Vec<LogMessage> {
    vec![
        said(1, "planner", "We should canary at 5%."),
        aside_row(2, "planner", &["auditor"], "Between us: I am not sure."),
        aside_row(
            3,
            "auditor",
            &["planner"],
            "Nor am I. The rollback is the risk.",
        ),
        aside_row(4, "planner", &["auditor"], "Agreed. I will say so."),
        said(5, "planner", "The rollback path is the real risk here."),
        said(6, "archivist", "We hit that in March."),
    ]
}

#[test]
fn a_member_reads_an_aside_in_full() {
    let projected = as_viewer(desk_transcript(), agent("auditor"));
    assert_eq!(projected.len(), 6);
    assert!(projected.iter().all(|message| message.elided.is_none()));
    assert_eq!(projected[1].readable(), Some("Between us: I am not sure."),);
}

#[test]
fn its_author_reads_its_own_aside() {
    let projected = as_viewer(desk_transcript(), agent("planner"));
    assert!(projected.iter().all(|message| message.elided.is_none()));
}

#[test]
fn a_person_and_the_operator_read_every_aside_in_full() {
    for viewer in [Viewer::Operator, Viewer::Person { id: "ada".into() }] {
        let projected = as_viewer(desk_transcript(), viewer.clone());
        assert!(
            projected.iter().all(|message| message.elided.is_none()),
            "{viewer:?} must read everything",
        );
    }
}

#[test]
fn a_non_member_sees_one_collapsed_stub_rather_than_the_content() {
    let projected = as_viewer(desk_transcript(), agent("archivist"));

    // Six rows become four: the three-row aside is one stub.
    assert_eq!(
        projected
            .iter()
            .map(|message| message.sequence)
            .collect::<Vec<_>>(),
        vec![Sequence(1), Sequence(2), Sequence(5), Sequence(6)],
    );

    let stub = &projected[1];
    assert_eq!(stub.readable(), None);
    assert!(stub.content.is_empty(), "{:?}", stub.content);
    // Attribution survives: the row still says who spoke and to whom.
    assert_eq!(
        stub.author,
        SessionAuthor::Agent {
            id: "planner".into(),
            label: "planner".into(),
        },
    );
    assert_eq!(stub.audience.members(), ["auditor".to_owned()]);

    let elision = stub.elided.as_ref().expect("a stub");
    assert_eq!(elision.through, Sequence(4));
    assert_eq!(elision.messages, 3);
}

#[test]
fn a_stub_points_at_where_the_aside_settled() {
    let projected = as_viewer(desk_transcript(), agent("archivist"));
    let elision = projected[1].elided.as_ref().expect("a stub");
    // Sequence 5 is the first thing a participant said in the open afterwards,
    // and it is a row this viewer can read. That is what turns a hole in a
    // reader's context into a resolvable pointer.
    assert_eq!(elision.settled_at, Some(Sequence(5)));
    assert_eq!(
        projected[2].readable(),
        Some("The rollback path is the real risk here."),
    );
}

#[test]
fn an_unsettled_aside_says_so_rather_than_pointing_nowhere() {
    let mut rows = desk_transcript();
    rows.truncate(4);
    let projected = as_viewer(rows, agent("archivist"));
    let elision = projected[1].elided.as_ref().expect("a stub");
    assert_eq!(elision.settled_at, None);
}

#[test]
fn a_settlement_by_either_participant_counts() {
    let rows = vec![
        aside_row(1, "planner", &["auditor"], "privately"),
        said(2, "auditor", "I checked, and the rollback is the risk."),
    ];
    let projected = as_viewer(rows, agent("archivist"));
    assert_eq!(
        projected[0].elided.as_ref().expect("a stub").settled_at,
        Some(Sequence(2)),
    );
}

#[test]
fn a_non_participants_message_does_not_settle_an_aside() {
    let rows = vec![
        aside_row(1, "planner", &["auditor"], "privately"),
        said(2, "archivist", "Unrelated."),
        said(3, "planner", "Here is what we concluded."),
    ];
    let projected = as_viewer(rows, agent("archivist"));
    assert_eq!(
        projected[0].elided.as_ref().expect("a stub").settled_at,
        Some(Sequence(3)),
    );
}

#[test]
fn a_later_private_row_never_settles_an_earlier_aside() {
    // `auditor` is a participant of the first aside, and later addresses the
    // viewer directly in a second, separate aside. That second row is
    // unelided for this viewer — it is inside its audience — but it is still
    // `Audience::Aside`, not desk-visible, so it must not be mistaken for the
    // open settlement the first aside owes the room.
    let rows = vec![
        aside_row(1, "planner", &["auditor"], "privately"),
        aside_row(2, "auditor", &["archivist"], "a different private aside"),
    ];
    let projected = as_viewer(rows, agent("archivist"));
    assert_eq!(projected[1].readable(), Some("a different private aside"));
    assert_eq!(
        projected[0].elided.as_ref().expect("a stub").settled_at,
        None,
    );
}

#[test]
fn two_separate_asides_do_not_collapse_into_one() {
    let rows = vec![
        aside_row(1, "planner", &["auditor"], "one"),
        aside_row(2, "auditor", &["planner"], "two"),
        aside_row(3, "planner", &["archivist"], "a different pair"),
    ];
    let projected = as_viewer(rows, agent("scribe"));
    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].elided.as_ref().expect("a stub").messages, 2);
    assert_eq!(projected[1].elided.as_ref().expect("a stub").messages, 1);
}

#[test]
fn a_reply_in_the_other_direction_belongs_to_the_same_aside() {
    // `planner -> auditor` and `auditor -> planner` name different audiences
    // and the same participants, so they are one exchange.
    let rows = vec![
        aside_row(1, "planner", &["auditor"], "one"),
        aside_row(2, "auditor", &["planner"], "two"),
    ];
    let projected = as_viewer(rows, agent("archivist"));
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].elided.as_ref().expect("a stub").messages, 2);
}

#[test]
fn an_aside_split_by_a_desk_row_is_two_stubs() {
    let rows = vec![
        aside_row(1, "planner", &["auditor"], "one"),
        said(2, "archivist", "interrupting"),
        aside_row(3, "planner", &["auditor"], "two"),
    ];
    let projected = as_viewer(rows, agent("archivist"));
    assert_eq!(projected.len(), 3);
    assert!(projected[0].elided.is_some());
    assert!(projected[1].elided.is_none());
    assert!(projected[2].elided.is_some());
}

#[test]
fn a_transcript_with_no_aside_projects_identically_for_every_viewer() {
    let rows = vec![
        said(1, "planner", "one"),
        said(2, "auditor", "two"),
        said(3, "archivist", "three"),
    ];
    let baseline = as_viewer(rows.clone(), Viewer::Operator);
    for viewer in [
        agent("planner"),
        agent("nobody"),
        Viewer::Person { id: "ada".into() },
    ] {
        assert_eq!(as_viewer(rows.clone(), viewer), baseline);
    }
}

#[test]
fn a_thread_scoped_projection_elides_rather_than_losing_its_root() {
    // A thread whose root is an aside must still terminate the walk at that
    // root. Eliding rather than dropping is what makes that fall out.
    let rows = vec![
        LogMessage {
            audience: Audience::Aside {
                members: vec!["auditor".into()],
            },
            ..said(1, "planner", "privately, about this")
        },
        LogMessage {
            parent: Some(Sequence(1)),
            audience: Audience::Aside {
                members: vec!["planner".into()],
            },
            ..said(2, "auditor", "privately, in reply")
        },
    ];
    let log = FakeLog::new(vec![page(rows.into_iter().rev().collect(), None)]);
    let query = SessionQuery {
        conversation: Conversation {
            thread_root: Some(Sequence(1)),
            ..conversation()
        },
        before: None,
        window: 30,
        viewer: agent("archivist"),
    };
    let projected = futures_lite_block_on(project_session(&log, &query));
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].sequence, Sequence(1));
    assert_eq!(projected[0].elided.as_ref().expect("a stub").messages, 2);
}

#[test]
fn project_as_narrows_a_transcript_a_caller_already_holds() {
    let held = as_viewer(desk_transcript(), Viewer::Operator);
    assert_eq!(project_as(&held, &agent("archivist")).len(), 4);
    assert_eq!(project_as(&held, &agent("auditor")).len(), 6);
}

#[test]
fn project_as_leaves_an_already_elided_row_alone() {
    let narrowed = project_as(
        &as_viewer(desk_transcript(), Viewer::Operator),
        &agent("archivist"),
    );
    // Narrowing twice must not re-elide the stub and lose the run it stands
    // for, which is what a naive second pass would do.
    assert_eq!(project_as(&narrowed, &agent("archivist")), narrowed);
    assert_eq!(narrowed[1].elided.as_ref().expect("a stub").messages, 3);
}

#[test]
fn two_closed_threads_between_the_same_pair_stay_two_stubs() {
    // Both roots are asides between the same participants, and each owns a
    // reply. Collapsing on participants alone would report one stub spanning
    // both, with a combined range and count for exchanges that never were one
    // exchange.
    let rows = vec![
        aside_row(1, "planner", &["auditor"], "first thread, root"),
        LogMessage {
            parent: Some(Sequence(1)),
            ..aside_row(2, "auditor", &["planner"], "first thread, reply")
        },
        aside_row(3, "planner", &["auditor"], "second thread, root"),
        LogMessage {
            parent: Some(Sequence(3)),
            ..aside_row(4, "auditor", &["planner"], "second thread, reply")
        },
    ];
    let projected = as_viewer(rows, agent("archivist"));
    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].sequence, Sequence(1));
    assert_eq!(
        projected[0].elided.as_ref().expect("a stub").through,
        Sequence(2)
    );
    assert_eq!(projected[1].sequence, Sequence(3));
    assert_eq!(
        projected[1].elided.as_ref().expect("a stub").through,
        Sequence(4)
    );
}

#[test]
fn a_run_of_channel_asides_is_still_one_stub() {
    // The other half of the same rule: rows nothing replies to are ordinary
    // channel messages, so a run of them between one pair is one exchange.
    let rows = vec![
        aside_row(1, "planner", &["auditor"], "one"),
        aside_row(2, "auditor", &["planner"], "two"),
        aside_row(3, "planner", &["auditor"], "three"),
    ];
    let projected = as_viewer(rows, agent("archivist"));
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].elided.as_ref().expect("a stub").messages, 3);
}

#[test]
fn a_settlement_narrowing_dropped_still_settles_the_stub() {
    // Channel narrowing keeps a root and its *first* reply. Here the first
    // reply is the aside and the settlement is the second, so the settlement
    // is not in the projection at all — it has to be found in the scanned
    // slice or the stub reports `settled_at: None` for an aside the room did
    // settle.
    let rows = vec![
        said(1, "planner", "How should we cut over?"),
        LogMessage {
            parent: Some(Sequence(1)),
            ..aside_row(2, "auditor", &["planner"], "privately, I am unsure")
        },
        LogMessage {
            parent: Some(Sequence(1)),
            ..said(3, "auditor", "The rollback path is the risk.")
        },
    ];
    let projected = as_viewer(rows, agent("archivist"));
    let stub = projected
        .iter()
        .find(|message| message.elided.is_some())
        .expect("a stub");
    assert_eq!(
        stub.elided.as_ref().expect("a stub").settled_at,
        Some(Sequence(3)),
    );
    // And the settlement itself is genuinely absent from this projection,
    // which is what makes the assertion above non-trivial.
    assert!(
        !projected
            .iter()
            .any(|message| message.sequence == Sequence(3))
    );
}

#[test]
fn a_settlement_before_the_aside_does_not_settle_it() {
    // `settled_at` points at what a participant said *afterwards*. A row
    // earlier in the transcript is not an outcome of an exchange that had not
    // happened yet.
    let rows = vec![
        said(1, "planner", "An opinion, stated before anything private."),
        aside_row(2, "planner", &["auditor"], "and now privately"),
    ];
    let projected = as_viewer(rows, agent("archivist"));
    assert_eq!(
        projected[1].elided.as_ref().expect("a stub").settled_at,
        None,
    );
}
