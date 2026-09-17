//! Approval gate call-count, mapping, and answer-validation tests.

#![allow(clippy::unwrap_used)]

use std::{
    io,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;
use tinyhivemind_core::{
    approval::{Action, ActionTarget, Effect, Millis},
    dispatch::DispatchConversation,
};

struct Gate {
    calls: AtomicUsize,
    outcome: std::result::Result<AskOutcome, ()>,
}

impl ApprovalGate for Gate {
    fn ask_once(&self, _: ApprovalPrompt) -> ApprovalFuture<'_> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            self.outcome
                .clone()
                .map_err(|()| Box::new(io::Error::other("gate failed")) as BoxError)
        })
    }
}

fn request() -> ApprovalRequest {
    ApprovalRequest {
        epoch: ConsentEpoch(2),
        sequence: 9,
        call_id: "call".into(),
        actor_id: "agent".into(),
        conversation: DispatchConversation {
            desk_id: "engineering".into(),
            thread_root: None,
        },
        action: Action {
            verb: "write".into(),
            target: ActionTarget::Resource {
                path: "/repo/file".into(),
            },
            effect: Effect::Mutating,
        },
    }
}

fn ask(request: &ApprovalRequest) -> ApprovalDecision {
    ask_with_scope(request, GrantScope::Call)
}

fn ask_with_scope(request: &ApprovalRequest, scope: GrantScope) -> ApprovalDecision {
    ApprovalDecision::Ask {
        who: "operator".into(),
        scope,
        key: ScopeKey::for_request(request),
        epoch: request.epoch,
    }
}

#[tokio::test]
async fn narrower_grant_and_refusal_scopes_are_accepted() {
    let request = request();
    let offered = GrantScope::Resource {
        root: "/repo".into(),
    };
    for scope in [
        GrantScope::Call,
        GrantScope::Action,
        GrantScope::Resource {
            root: "/repo/file".into(),
        },
    ] {
        let gate = Gate {
            calls: AtomicUsize::new(0),
            outcome: Ok(AskOutcome::Answered {
                answer: ApprovalAnswer::Approved {
                    grant: Some(StandingGrant {
                        scope: scope.clone(),
                        key: ScopeKey::for_request(&request),
                        granted_at_epoch: request.epoch,
                        granted_at_sequence: request.sequence,
                        granted_at: Millis(1),
                        expires_at: None,
                        revoked: false,
                    }),
                },
            }),
        };
        assert!(matches!(
            request_approval(&gate, &request, ask_with_scope(&request, offered.clone()))
                .await
                .unwrap(),
            ApprovalOutcome::Approved { .. }
        ));

        let gate = Gate {
            calls: AtomicUsize::new(0),
            outcome: Ok(AskOutcome::Answered {
                answer: ApprovalAnswer::Refused {
                    refusal: Some(RememberedRefusal {
                        scope,
                        key: ScopeKey::for_request(&request),
                        epoch: request.epoch,
                    }),
                },
            }),
        };
        assert!(matches!(
            request_approval(&gate, &request, ask_with_scope(&request, offered.clone()))
                .await
                .unwrap(),
            ApprovalOutcome::Refused { .. }
        ));
    }
}

#[tokio::test]
async fn allow_and_deny_call_the_gate_zero_times() {
    let gate = Gate {
        calls: AtomicUsize::new(0),
        outcome: Ok(AskOutcome::Asked),
    };
    let allowed = request_approval(
        &gate,
        &request(),
        ApprovalDecision::Allow {
            basis: AllowBasis::Policy,
        },
    )
    .await
    .unwrap();
    assert!(matches!(allowed, ApprovalOutcome::Allowed { .. }));
    let denied = request_approval(
        &gate,
        &request(),
        ApprovalDecision::Deny {
            reason: DenyReason::NoRule,
        },
    )
    .await
    .unwrap();
    assert!(matches!(denied, ApprovalOutcome::Denied { .. }));
    assert_eq!(gate.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn ask_calls_once_and_maps_each_host_status() {
    for (ask_outcome, expected) in [
        (AskOutcome::Asked, ApprovalOutcome::Asked),
        (AskOutcome::Already, ApprovalOutcome::Already),
    ] {
        let gate = Gate {
            calls: AtomicUsize::new(0),
            outcome: Ok(ask_outcome),
        };
        assert_eq!(
            request_approval(&gate, &request(), ask(&request()))
                .await
                .unwrap(),
            expected
        );
        assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn exact_grant_is_accepted_and_widened_grant_is_rejected() {
    let request = request();
    let exact = StandingGrant {
        scope: GrantScope::Call,
        key: ScopeKey::for_request(&request),
        granted_at_epoch: request.epoch,
        granted_at_sequence: 10,
        granted_at: Millis(1),
        expires_at: None,
        revoked: false,
    };
    let gate = Gate {
        calls: AtomicUsize::new(0),
        outcome: Ok(AskOutcome::Answered {
            answer: ApprovalAnswer::Approved {
                grant: Some(exact.clone()),
            },
        }),
    };
    assert!(matches!(
        request_approval(&gate, &request, ask(&request))
            .await
            .unwrap(),
        ApprovalOutcome::Approved { .. }
    ));

    let gate = Gate {
        calls: AtomicUsize::new(0),
        outcome: Ok(AskOutcome::Answered {
            answer: ApprovalAnswer::Approved {
                grant: Some(StandingGrant {
                    scope: GrantScope::Action,
                    ..exact
                }),
            },
        }),
    };
    assert!(matches!(
        request_approval(&gate, &request, ask(&request)).await,
        Err(crate::Error::InvalidApprovalAnswer)
    ));
}

#[tokio::test]
async fn exact_refusal_is_accepted_and_mismatched_refusal_is_rejected() {
    let request = request();
    let exact = RememberedRefusal {
        scope: GrantScope::Call,
        key: ScopeKey::for_request(&request),
        epoch: request.epoch,
    };
    let gate = Gate {
        calls: AtomicUsize::new(0),
        outcome: Ok(AskOutcome::Answered {
            answer: ApprovalAnswer::Refused {
                refusal: Some(exact.clone()),
            },
        }),
    };
    assert!(matches!(
        request_approval(&gate, &request, ask(&request))
            .await
            .unwrap(),
        ApprovalOutcome::Refused { .. }
    ));

    let gate = Gate {
        calls: AtomicUsize::new(0),
        outcome: Ok(AskOutcome::Answered {
            answer: ApprovalAnswer::Refused {
                refusal: Some(RememberedRefusal {
                    epoch: ConsentEpoch(99),
                    ..exact
                }),
            },
        }),
    };
    assert!(matches!(
        request_approval(&gate, &request, ask(&request)).await,
        Err(crate::Error::InvalidApprovalAnswer)
    ));
}

#[tokio::test]
async fn host_failure_is_preserved_as_approval_gate_error() {
    let gate = Gate {
        calls: AtomicUsize::new(0),
        outcome: Err(()),
    };
    assert!(matches!(
        request_approval(&gate, &request(), ask(&request())).await,
        Err(crate::Error::ApprovalGate { .. })
    ));
}

#[test]
fn prompt_wire_includes_the_scoped_dedupe_key() {
    let request = request();
    let key = ScopeKey::for_request(&request);
    let prompt = ApprovalPrompt {
        who: "operator".into(),
        scope: GrantScope::Call,
        key: key.clone(),
        epoch: request.epoch,
        request_sequence: request.sequence,
        dedupe_key: super::dedupe_key(&key, request.sequence),
    };
    let value = serde_json::to_value(prompt).unwrap();
    assert_eq!(value["request_sequence"], 9);
    assert!(value["dedupe_key"].as_str().unwrap().ends_with("s1:9"));
}
