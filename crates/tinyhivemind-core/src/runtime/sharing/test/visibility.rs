//! Private-aside visibility tests for `prepare_delta`: it applies the same
//! audience predicate the projection does, a member reads its own aside, and
//! a re-seed never hands a member less than the delta already did.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use super::support::{FakeLog, delta, engineering, page, raw, state};
use crate::aside::Audience;
use crate::aside::Viewer;
use crate::runtime::LogMessage;

fn said(sequence: u64, id: &str, content: &str) -> LogMessage {
    LogMessage {
        author: crate::runtime::SessionAuthor::Agent {
            id: id.into(),
            label: id.into(),
        },
        ..raw(sequence, Some("engineering"), None, content)
    }
}

fn aside_row(sequence: u64, id: &str, members: &[&str], content: &str) -> LogMessage {
    LogMessage {
        audience: Audience::Aside {
            members: members.iter().map(|member| (*member).to_owned()).collect(),
        },
        ..said(sequence, id, content)
    }
}

async fn plan_for(
    log: &FakeLog,
    state: &SharingState,
    before: u64,
    viewer: &Viewer,
) -> SharingPlan {
    let desired = engineering();
    let current = engineering();
    prepare_delta(
        log,
        &SharingQuery {
            desired_conversation: &desired,
            current_conversation: &current,
            state,
            before: Sequence(before),
            viewer,
        },
    )
    .await
    .expect("prepares")
}

fn tick_rows() -> Vec<LogMessage> {
    vec![
        said(13, "archivist", "in the open"),
        aside_row(12, "auditor", &["planner"], "and privately, in reply"),
        aside_row(11, "planner", &["auditor"], "privately"),
        // At the watermark, so the backward walk crosses it and the delta is
        // a delta rather than a request to re-seed.
        said(10, "planner", "already delivered"),
    ]
}

#[tokio::test]
async fn a_delta_applies_the_same_audience_rule_as_the_projection() {
    let log = FakeLog::new(vec![page(tick_rows(), None)]);
    let plan = plan_for(
        &log,
        &state(10),
        14,
        &Viewer::Agent {
            id: "archivist".into(),
        },
    )
    .await;
    let delta = delta(plan);
    // Two rows: one stub standing for the pair, and the desk row.
    assert_eq!(delta.messages.len(), 2);
    assert_eq!(delta.messages[0].readable(), None);
    assert_eq!(delta.messages[0].sequence, Sequence(11));
    assert_eq!(
        delta.messages[0].elided.as_ref().expect("a stub").messages,
        2,
    );
    assert_eq!(delta.messages[1].readable(), Some("in the open"));
}

#[tokio::test]
async fn a_member_receives_its_own_aside_in_a_delta() {
    let log = FakeLog::new(vec![page(tick_rows(), None)]);
    let plan = plan_for(
        &log,
        &state(10),
        14,
        &Viewer::Agent {
            id: "auditor".into(),
        },
    )
    .await;
    let delta = delta(plan);
    assert_eq!(delta.messages.len(), 3);
    assert!(delta.messages.iter().all(|m| m.elided.is_none()));
}

#[tokio::test]
async fn a_reseed_never_hands_a_member_less_than_the_delta_did() {
    // The incremental path and the projection have to agree, or an agent that
    // re-seeds sees a different transcript from one that stayed incremental
    // and nothing heals the difference.
    for id in ["planner", "archivist"] {
        let viewer = Viewer::Agent { id: id.into() };

        let log = FakeLog::new(vec![page(tick_rows(), None)]);
        let incremental = delta(plan_for(&log, &state(10), 14, &viewer).await).messages;

        let log = FakeLog::new(vec![page(tick_rows(), None)]);
        let reseeded = crate::runtime::project_session(
            &log,
            &crate::runtime::SessionQuery {
                conversation: engineering(),
                before: Some(Sequence(14)),
                window: 30,
                viewer: viewer.clone(),
            },
        )
        .await
        .expect("projects");

        // A re-seed projects the whole window and a delta only what is above
        // the watermark, so the comparison is over the rows they share.
        assert_eq!(
            incremental
                .iter()
                .map(|m| (m.sequence, m.readable().map(str::to_owned)))
                .collect::<Vec<_>>(),
            reseeded
                .iter()
                .filter(|m| m.sequence > Sequence(10))
                .map(|m| (m.sequence, m.readable().map(str::to_owned)))
                .collect::<Vec<_>>(),
            "{id} must read the same rows either way",
        );
    }
}

#[tokio::test]
async fn a_delta_over_rows_with_no_aside_is_the_same_for_every_viewer() {
    let rows = vec![
        said(12, "planner", "two"),
        said(11, "planner", "one"),
        said(10, "planner", "already delivered"),
    ];
    let baseline = {
        let log = FakeLog::new(vec![page(rows.clone(), None)]);
        delta(plan_for(&log, &state(10), 13, &Viewer::Operator).await).messages
    };
    for viewer in [
        Viewer::Agent {
            id: "anyone".into(),
        },
        Viewer::Person { id: "ada".into() },
    ] {
        let log = FakeLog::new(vec![page(rows.clone(), None)]);
        assert_eq!(
            delta(plan_for(&log, &state(10), 13, &viewer).await).messages,
            baseline
        );
    }
}
