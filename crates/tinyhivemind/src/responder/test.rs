//! Unit tests for the narrow runtime selector boundary.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use std::{
    io,
    sync::atomic::{AtomicUsize, Ordering},
};
use tinyhivemind_core::{
    desk::{Desk, DeskSet, ResponderMode},
    responder::{ResponderRequest, SelectionPolicy, SelectorCandidate},
    roster::{Roster, RosterMember},
};

struct StubSelector {
    calls: AtomicUsize,
    output: std::result::Result<SelectionEvaluation, ()>,
}

impl StubSelector {
    fn returning(output: SelectionEvaluation) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            output: Ok(output),
        }
    }

    fn failing() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            output: Err(()),
        }
    }
}

impl Selector for StubSelector {
    fn select<'a>(&'a self, _: &'a SelectionRequest) -> SelectorFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            self.output
                .clone()
                .map_err(|()| Box::new(io::Error::other("selector failed")) as BoxError)
        })
    }
}

struct BorrowingSelector;

impl Selector for BorrowingSelector {
    fn select<'a>(&'a self, request: &'a SelectionRequest) -> SelectorFuture<'a> {
        Box::pin(async move {
            tokio::task::yield_now().await;
            Ok(evaluation(if request.message == "Review this" {
                "bob"
            } else {
                "alice"
            }))
        })
    }
}

fn fixture() -> (
    Vec<RosterMember>,
    Vec<Desk>,
    ResponderRequest,
    Vec<SelectorCandidate>,
) {
    (
        vec![
            RosterMember {
                id: "orch".into(),
                name: None,
            },
            RosterMember {
                id: "alice".into(),
                name: Some("Alice".into()),
            },
            RosterMember {
                id: "bob".into(),
                name: Some("Bob".into()),
            },
        ],
        vec![Desk {
            id: "eng".into(),
            name: "Engineering".into(),
            description: None,
            members: vec!["alice".into(), "bob".into()],
            responder_mode: ResponderMode::Auto,
        }],
        ResponderRequest {
            message: "Review this".into(),
            chat: Some("eng".into()),
            mentions: Vec::new(),
            orchestrator_id: "orch".into(),
            selection_policy: SelectionPolicy::Allowed,
            minimum_selection_confidence: Probability::new(500_000).unwrap(),
        },
        vec![
            SelectorCandidate {
                id: "alice".into(),
                label: "Alice".into(),
                role: "Builder".into(),
                description: None,
            },
            SelectorCandidate {
                id: "bob".into(),
                label: "Bob".into(),
                role: "Reviewer".into(),
                description: None,
            },
        ],
    )
}

fn evaluation(choice: &str) -> SelectionEvaluation {
    SelectionEvaluation {
        choice: choice.to_owned(),
        probabilities: vec![
            CandidateProbability {
                candidate_id: "alice".into(),
                probability: if choice == "alice" {
                    Probability::new(800_000).unwrap()
                } else {
                    Probability::new(200_000).unwrap()
                },
            },
            CandidateProbability {
                candidate_id: "bob".into(),
                probability: if choice == "bob" {
                    Probability::new(800_000).unwrap()
                } else {
                    Probability::new(200_000).unwrap()
                },
            },
        ],
        confidence: Probability::new(600_000).unwrap(),
    }
}

#[tokio::test]
async fn valid_selector_output_is_called_once_and_selects_one_agent() {
    let (members, records, request, details) = fixture();
    let roster = Roster::new(&members, &[], &[]);
    let desks = DeskSet::new(&records, &[], &[], &[], &[]);
    let selector = StubSelector::returning(evaluation("bob"));
    let selected = choose_responder(Some(&selector), &request, &roster, &desks, &details)
        .await
        .unwrap();
    assert_eq!(selector.calls.load(Ordering::SeqCst), 1);
    assert_eq!(selected.responder_id, "bob");
    assert_eq!(selected.rung, ResponderRung::AutoSelection);
    assert_eq!(selected.disposition, SelectionDisposition::Selected);
}

#[tokio::test]
async fn selector_may_borrow_the_request_across_an_await() {
    let (members, records, request, details) = fixture();
    let roster = Roster::new(&members, &[], &[]);
    let desks = DeskSet::new(&records, &[], &[], &[], &[]);
    let selected = choose_responder(
        Some(&BorrowingSelector),
        &request,
        &roster,
        &desks,
        &details,
    )
    .await
    .unwrap();
    assert_eq!(selected.responder_id, "bob");
    assert_eq!(selected.rung, ResponderRung::AutoSelection);
}

#[tokio::test]
async fn absent_selector_uses_unavailable_desk_default_without_a_call() {
    let (members, records, request, details) = fixture();
    let roster = Roster::new(&members, &[], &[]);
    let desks = DeskSet::new(&records, &[], &[], &[], &[]);
    let selected = choose_responder(None, &request, &roster, &desks, &details)
        .await
        .unwrap();
    assert_eq!(selected.responder_id, "alice");
    assert_eq!(selected.rung, ResponderRung::DeskDefault);
    assert_eq!(selected.disposition, SelectionDisposition::Unavailable);
}

#[tokio::test]
async fn selector_failure_uses_unavailable_desk_default_after_one_call() {
    let (members, records, request, details) = fixture();
    let roster = Roster::new(&members, &[], &[]);
    let desks = DeskSet::new(&records, &[], &[], &[], &[]);
    let selector = StubSelector::failing();
    let selected = choose_responder(Some(&selector), &request, &roster, &desks, &details)
        .await
        .unwrap();
    assert_eq!(selector.calls.load(Ordering::SeqCst), 1);
    assert_eq!(selected.rung, ResponderRung::DeskDefault);
    assert_eq!(selected.disposition, SelectionDisposition::Unavailable);
}

#[tokio::test]
async fn invalid_selector_output_uses_invalid_output_desk_default() {
    let (members, records, request, details) = fixture();
    let roster = Roster::new(&members, &[], &[]);
    let desks = DeskSet::new(&records, &[], &[], &[], &[]);
    let mut invalid = evaluation("bob");
    invalid.probabilities.pop();
    let selector = StubSelector::returning(invalid);
    let selected = choose_responder(Some(&selector), &request, &roster, &desks, &details)
        .await
        .unwrap();
    assert_eq!(selector.calls.load(Ordering::SeqCst), 1);
    assert_eq!(selected.responder_id, "alice");
    assert_eq!(selected.rung, ResponderRung::DeskDefault);
    assert_eq!(selected.disposition, SelectionDisposition::InvalidOutput);
}

#[tokio::test]
async fn immediate_decision_never_calls_selector() {
    let (members, records, mut request, details) = fixture();
    request.chat = None;
    let roster = Roster::new(&members, &[], &[]);
    let desks = DeskSet::new(&records, &[], &[], &[], &[]);
    let selector = StubSelector::returning(evaluation("bob"));
    let selected = choose_responder(Some(&selector), &request, &roster, &desks, &details)
        .await
        .unwrap();
    assert_eq!(selector.calls.load(Ordering::SeqCst), 0);
    assert_eq!(selected.responder_id, "orch");
}
