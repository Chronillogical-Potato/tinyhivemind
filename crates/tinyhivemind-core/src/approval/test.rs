//! Approval failure, grant, epoch, rendering, and wire behavior.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::{
    desk::{Desk, DeskSet, ResponderMode},
    dispatch::DispatchConversation,
    roster::{Person, Roster, RosterMember},
};

struct Fixture {
    members: Vec<RosterMember>,
    people: Vec<Person>,
    desks: Vec<Desk>,
    request: ApprovalRequest,
    policy: ApprovalPolicy,
}

impl Fixture {
    fn new() -> Self {
        Self {
            members: vec![RosterMember {
                id: "agent".into(),
                name: Some("Agent".into()),
            }],
            people: vec![Person {
                id: "operator".into(),
                label: "Operator".into(),
            }],
            desks: vec![Desk {
                id: "engineering".into(),
                name: "Engineering".into(),
                description: None,
                members: vec!["agent".into()],
                responder_mode: ResponderMode::Lead,
            }],
            request: ApprovalRequest {
                epoch: ConsentEpoch(3),
                sequence: 20,
                call_id: "call-1".into(),
                actor_id: "agent".into(),
                conversation: DispatchConversation {
                    desk_id: "engineering".into(),
                    thread_root: None,
                },
                action: Action {
                    verb: "write".into(),
                    target: ActionTarget::Resource {
                        path: "/repo/src/lib.rs".into(),
                    },
                    effect: Effect::Mutating,
                },
            },
            policy: ApprovalPolicy {
                enabled: true,
                default: DefaultVerdict::Deny,
                rules: vec![ApprovalRule {
                    effect: Some(Effect::Mutating),
                    verb: Some("write".into()),
                    target: None,
                    verdict: RuleVerdict::Allow,
                }],
                approver: ApproverRule::Person {
                    id: "operator".into(),
                },
                allow_grants: true,
                max_grant_ttl: Some(Millis(1_000)),
            },
        }
    }

    fn decide(&self, grants: &[StandingGrant], refusals: &[RememberedRefusal]) -> ApprovalDecision {
        let roster = Roster::new(&self.members, &self.people, &[]);
        let desks = DeskSet::new(&self.desks, &[], &[], &[], &[]);
        approve(
            &self.request,
            &self.policy,
            grants,
            refusals,
            &roster,
            &desks,
            Millis(100),
        )
    }
}

fn grant(fixture: &Fixture, scope: GrantScope) -> StandingGrant {
    StandingGrant {
        scope,
        key: ScopeKey::for_request(&fixture.request),
        granted_at_epoch: ConsentEpoch(2),
        granted_at_sequence: 10,
        granted_at: Millis(50),
        expires_at: Some(Millis(500)),
        revoked: false,
    }
}

#[test]
fn a_matching_allow_rule_allows() {
    assert_eq!(
        Fixture::new().decide(&[], &[]),
        ApprovalDecision::Allow {
            basis: AllowBasis::Policy
        }
    );
}

#[test]
fn every_precondition_fails_closed() {
    let mut fixture = Fixture::new();
    fixture.policy.enabled = false;
    assert_eq!(
        fixture.decide(&[], &[]),
        ApprovalDecision::Deny {
            reason: DenyReason::Disabled
        }
    );

    let mut fixture = Fixture::new();
    fixture.request.call_id.clear();
    assert_eq!(
        fixture.decide(&[], &[]),
        ApprovalDecision::Deny {
            reason: DenyReason::MalformedRequest
        }
    );

    let mut fixture = Fixture::new();
    fixture.request.actor_id = "retired".into();
    assert_eq!(
        fixture.decide(&[], &[]),
        ApprovalDecision::Deny {
            reason: DenyReason::UnknownActor
        }
    );

    let mut fixture = Fixture::new();
    fixture.request.action.effect = Effect::Unclassified;
    assert_eq!(
        fixture.decide(&[], &[]),
        ApprovalDecision::Deny {
            reason: DenyReason::UnclassifiedAction
        }
    );
}

#[test]
fn deny_beats_allow_regardless_of_rule_order() {
    let mut fixture = Fixture::new();
    fixture.policy.rules.push(ApprovalRule {
        effect: None,
        verb: None,
        target: None,
        verdict: RuleVerdict::Deny,
    });
    assert_eq!(
        fixture.decide(&[], &[]),
        ApprovalDecision::Deny {
            reason: DenyReason::PolicyDenied
        }
    );
    fixture.policy.rules.reverse();
    assert_eq!(
        fixture.decide(&[], &[]),
        ApprovalDecision::Deny {
            reason: DenyReason::PolicyDenied
        }
    );
}

#[test]
fn same_epoch_refusal_beats_a_covering_grant() {
    let fixture = Fixture::new();
    let key = ScopeKey::for_request(&fixture.request);
    let refusal = RememberedRefusal {
        key,
        scope: GrantScope::Call,
        epoch: fixture.request.epoch,
    };
    assert_eq!(
        fixture.decide(&[grant(&fixture, GrantScope::Call)], &[refusal]),
        ApprovalDecision::Deny {
            reason: DenyReason::RememberedRefusal
        }
    );
}

#[test]
fn refusal_retires_but_grant_survives_an_epoch_change() {
    let fixture = Fixture::new();
    let refusal = RememberedRefusal {
        key: ScopeKey::for_request(&fixture.request),
        scope: GrantScope::Call,
        epoch: ConsentEpoch(2),
    };
    assert!(matches!(
        fixture.decide(&[grant(&fixture, GrantScope::Call)], &[refusal]),
        ApprovalDecision::Allow {
            basis: AllowBasis::Grant { .. }
        }
    ));
}

#[test]
fn consent_is_not_retroactive_within_an_epoch() {
    let fixture = Fixture::new();
    let mut later = grant(&fixture, GrantScope::Call);
    later.granted_at_epoch = fixture.request.epoch;
    later.granted_at_sequence = fixture.request.sequence + 1;
    assert_eq!(
        fixture.decide(&[later], &[]),
        ApprovalDecision::Allow {
            basis: AllowBasis::Policy
        }
    );
}

#[test]
fn revoked_expired_future_and_overlong_grants_are_ignored() {
    let fixture = Fixture::new();
    let mut grants = Vec::new();
    let mut revoked = grant(&fixture, GrantScope::Call);
    revoked.revoked = true;
    grants.push(revoked);
    let mut expired = grant(&fixture, GrantScope::Call);
    expired.expires_at = Some(Millis(100));
    grants.push(expired);
    let mut future = grant(&fixture, GrantScope::Call);
    future.granted_at = Millis(101);
    grants.push(future);
    let mut overlong = grant(&fixture, GrantScope::Call);
    overlong.expires_at = Some(Millis(2_000));
    grants.push(overlong);
    assert_eq!(
        fixture.decide(&grants, &[]),
        ApprovalDecision::Allow {
            basis: AllowBasis::Policy
        }
    );
}

#[test]
fn resource_scope_contains_descendants_not_siblings() {
    let fixture = Fixture::new();
    let mut resource = grant(
        &fixture,
        GrantScope::Resource {
            root: "/repo/src".into(),
        },
    );
    resource.key.target = ActionTarget::Resource {
        path: "/repo/src".into(),
    };
    assert!(matches!(
        fixture.decide(&[resource.clone()], &[]),
        ApprovalDecision::Allow {
            basis: AllowBasis::Grant { .. }
        }
    ));
    let mut sibling = Fixture::new();
    sibling.request.action.target = ActionTarget::Resource {
        path: "/repo/tests/test.rs".into(),
    };
    sibling.policy.rules.clear();
    assert_eq!(
        sibling.decide(&[resource], &[]),
        ApprovalDecision::Deny {
            reason: DenyReason::NoRule
        }
    );
}

#[test]
fn resource_scope_requires_the_grant_key_to_belong_to_its_root() {
    let mut fixture = Fixture::new();
    fixture.policy.rules.clear();
    let mut resource = grant(
        &fixture,
        GrantScope::Resource {
            root: "/repo/src".into(),
        },
    );
    resource.key.target = ActionTarget::Named {
        name: "/repo/src".into(),
    };
    assert_eq!(
        fixture.decide(&[resource], &[]),
        ApprovalDecision::Deny {
            reason: DenyReason::NoRule
        }
    );
}

#[test]
fn grants_never_cross_effect_classifications() {
    let mut fixture = Fixture::new();
    fixture.request.action.effect = Effect::ReadOnly;
    let read_only = grant(&fixture, GrantScope::Action);
    fixture.request.action.effect = Effect::Mutating;
    fixture.policy.rules.clear();
    assert_eq!(
        fixture.decide(&[read_only], &[]),
        ApprovalDecision::Deny {
            reason: DenyReason::NoRule
        }
    );
}

#[test]
fn narrowest_earliest_grant_is_the_deterministic_basis() {
    let fixture = Fixture::new();
    let action = grant(&fixture, GrantScope::Action);
    let mut call = grant(&fixture, GrantScope::Call);
    call.granted_at_sequence = 11;
    let expected = call.key.clone();
    assert_eq!(
        fixture.decide(&[action, call], &[]),
        ApprovalDecision::Allow {
            basis: AllowBasis::Grant { key: expected }
        }
    );
}

#[test]
fn asking_names_one_person_and_carries_call_scope() {
    let mut fixture = Fixture::new();
    fixture.policy.rules[0].verdict = RuleVerdict::Ask;
    assert_eq!(
        fixture.decide(&[], &[]),
        ApprovalDecision::Ask {
            who: "operator".into(),
            scope: GrantScope::Call,
            key: ScopeKey::for_request(&fixture.request),
            epoch: ConsentEpoch(3),
        }
    );
}

#[test]
fn approver_lookup_failures_share_rendering_but_not_variants() {
    let mut absent = Fixture::new();
    absent.policy.rules[0].verdict = RuleVerdict::Ask;
    absent.policy.approver = ApproverRule::Absent;
    assert_eq!(
        absent.decide(&[], &[]),
        ApprovalDecision::Deny {
            reason: DenyReason::NoApprover
        }
    );

    let mut no_person = Fixture::new();
    no_person.policy.rules[0].verdict = RuleVerdict::Ask;
    no_person.policy.approver = ApproverRule::Person {
        id: "missing".into(),
    };
    let no_person = no_person.decide(&[], &[]);
    assert_eq!(
        no_person,
        ApprovalDecision::Deny {
            reason: DenyReason::NoApprover
        }
    );

    let mut no_desk = Fixture::new();
    no_desk.policy.rules[0].verdict = RuleVerdict::Ask;
    no_desk.request.conversation.desk_id = "missing".into();
    no_desk.policy.approver = ApproverRule::PerDesk {
        default: "operator".into(),
        overrides: Vec::new(),
    };
    let no_desk = no_desk.decide(&[], &[]);
    assert_eq!(
        no_desk,
        ApprovalDecision::Deny {
            reason: DenyReason::UnresolvableApprover
        }
    );
    assert_eq!(
        DenyReason::NoApprover.to_string(),
        DenyReason::UnresolvableApprover.to_string()
    );
}

#[test]
fn scope_render_is_collision_free_and_target_tagged() {
    let first = ScopeKey {
        actor_id: "a\0b".into(),
        call_id: "c".into(),
        verb: "v".into(),
        effect: Effect::Mutating,
        target: ActionTarget::Named { name: "x".into() },
    };
    let second = ScopeKey {
        actor_id: "a".into(),
        call_id: "b\0c".into(),
        verb: "v".into(),
        effect: Effect::Mutating,
        target: ActionTarget::Named { name: "x".into() },
    };
    assert_ne!(first.render(), second.render());
    let resource = ScopeKey {
        target: ActionTarget::Resource { path: "x".into() },
        ..first.clone()
    };
    assert_ne!(first.render(), resource.render());
}

#[test]
fn wire_forms_are_explicit_and_round_trip() {
    let fixture = Fixture::new();
    let encoded = serde_json::to_value(&fixture.policy).unwrap();
    let decoded: ApprovalPolicy = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, fixture.policy);
    let decision = fixture.decide(&[], &[]);
    let encoded = serde_json::to_string(&decision).unwrap();
    assert_eq!(
        serde_json::from_str::<ApprovalDecision>(&encoded).unwrap(),
        decision
    );
}

#[test]
fn every_denial_has_safe_lowercase_rendering() {
    for reason in [
        DenyReason::Disabled,
        DenyReason::MalformedRequest,
        DenyReason::UnknownActor,
        DenyReason::UnclassifiedAction,
        DenyReason::PolicyDenied,
        DenyReason::RememberedRefusal,
        DenyReason::UnresolvableApprover,
        DenyReason::NoApprover,
        DenyReason::NoRule,
    ] {
        let rendered = reason.to_string();
        assert!(rendered.starts_with(|character: char| character.is_ascii_lowercase()));
        assert!(!rendered.ends_with('.'));
    }
}

#[test]
fn named_and_resource_patterns_do_not_cross_variants() {
    let mut fixture = Fixture::new();
    fixture.request.action.target = ActionTarget::Named {
        name: "production".into(),
    };
    fixture.policy.rules = vec![ApprovalRule {
        effect: None,
        verb: None,
        target: Some(TargetPattern::Named {
            name: "production".into(),
        }),
        verdict: RuleVerdict::Allow,
    }];
    assert!(matches!(
        fixture.decide(&[], &[]),
        ApprovalDecision::Allow { .. }
    ));
    fixture.policy.rules[0].target = Some(TargetPattern::Resource {
        root: "production".into(),
    });
    assert_eq!(
        fixture.decide(&[], &[]),
        ApprovalDecision::Deny {
            reason: DenyReason::NoRule
        }
    );
}

#[test]
fn perpetual_grants_follow_the_policy_cap() {
    let fixture = Fixture::new();
    let mut perpetual = grant(&fixture, GrantScope::Call);
    perpetual.expires_at = None;
    assert_eq!(
        fixture.decide(&[perpetual.clone()], &[]),
        ApprovalDecision::Allow {
            basis: AllowBasis::Policy
        }
    );
    let mut uncapped = Fixture::new();
    uncapped.policy.max_grant_ttl = None;
    assert!(matches!(
        uncapped.decide(&[perpetual], &[]),
        ApprovalDecision::Allow {
            basis: AllowBasis::Grant { .. }
        }
    ));
}

#[test]
fn grant_never_crosses_actor_or_verb_and_paths_keep_absolute_identity() {
    let mut fixture = Fixture::new();
    fixture.policy.rules.clear();
    let mut wrong_actor = grant(&fixture, GrantScope::Action);
    wrong_actor.key.actor_id = "other".into();
    assert!(matches!(
        fixture.decide(&[wrong_actor], &[]),
        ApprovalDecision::Deny { .. }
    ));
    let mut relative = grant(
        &fixture,
        GrantScope::Resource {
            root: "repo".into(),
        },
    );
    relative.key.target = ActionTarget::Resource {
        path: "repo".into(),
    };
    assert!(matches!(
        fixture.decide(&[relative], &[]),
        ApprovalDecision::Deny { .. }
    ));
}

#[test]
fn per_desk_override_selects_its_person() {
    let mut fixture = Fixture::new();
    fixture.people.push(Person {
        id: "lead".into(),
        label: "Lead".into(),
    });
    fixture.policy.rules[0].verdict = RuleVerdict::Ask;
    fixture.policy.approver = ApproverRule::PerDesk {
        default: "operator".into(),
        overrides: vec![DeskApprover {
            desk_id: "engineering".into(),
            person_id: "lead".into(),
        }],
    };
    assert!(matches!(
        fixture.decide(&[], &[]),
        ApprovalDecision::Ask { who, .. } if who == "lead"
    ));
}
