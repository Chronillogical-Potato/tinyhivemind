//! `approve`: a total gate over one side-effecting action.

use tinyhivemind_core::approval::{
    Action, ActionTarget, AllowBasis, ApprovalDecision, ApprovalPolicy, ApprovalRequest,
    ApprovalRule, ApproverRule, ConsentEpoch, DefaultVerdict, DeskApprover, Effect, GrantScope,
    Millis, RememberedRefusal, RuleVerdict, ScopeKey, StandingGrant, TargetPattern, approve,
};
use tinyhivemind_core::desk::{Desk, DeskSet, ResponderMode};
use tinyhivemind_core::dispatch::DispatchConversation;
use tinyhivemind_core::roster::{Person, Roster, RosterMember};
use tinyhivemind_lab::{Res, section};

fn request(
    actor: &str,
    desk: &str,
    verb: &str,
    target: ActionTarget,
    effect: Effect,
) -> ApprovalRequest {
    ApprovalRequest {
        epoch: ConsentEpoch(2),
        sequence: 50,
        call_id: "call-1".into(),
        actor_id: actor.into(),
        conversation: DispatchConversation {
            desk_id: desk.into(),
            thread_root: None,
        },
        action: Action {
            verb: verb.into(),
            target,
            effect,
        },
    }
}

fn resource(path: &str) -> ActionTarget {
    ActionTarget::Resource { path: path.into() }
}

fn policy() -> ApprovalPolicy {
    ApprovalPolicy {
        enabled: true,
        default: DefaultVerdict::Ask,
        rules: Vec::new(),
        approver: ApproverRule::Person { id: "pat".into() },
        allow_grants: true,
        max_grant_ttl: None,
    }
}

fn rule(
    effect: Option<Effect>,
    verb: Option<&str>,
    target: Option<TargetPattern>,
    verdict: RuleVerdict,
) -> ApprovalRule {
    ApprovalRule {
        effect,
        verb: verb.map(str::to_owned),
        target,
        verdict,
    }
}

fn show(decision: &ApprovalDecision) -> String {
    match decision {
        ApprovalDecision::Allow {
            basis: AllowBasis::Policy,
        } => "ALLOW (policy)".into(),
        ApprovalDecision::Allow {
            basis: AllowBasis::Grant { key },
        } => format!("ALLOW (grant {})", key.render()),
        ApprovalDecision::Deny { reason } => format!("DENY {reason:?}: {reason}"),
        ApprovalDecision::Ask {
            who, scope, epoch, ..
        } => format!("ASK {who} (scope {scope:?}, epoch {})", epoch.0),
    }
}

fn grant(scope: GrantScope, on: &ApprovalRequest, at: u64, expires: Option<u64>) -> StandingGrant {
    StandingGrant {
        scope,
        key: ScopeKey::for_request(on),
        granted_at_epoch: ConsentEpoch(1),
        granted_at_sequence: 10,
        granted_at: Millis(at),
        expires_at: expires.map(Millis),
        revoked: false,
    }
}

pub fn run() -> Res {
    section("approve: every verdict and every denial");
    let members = [
        RosterMember {
            id: "ada".into(),
            name: None,
        },
        RosterMember {
            id: "ben".into(),
            name: None,
        },
        RosterMember {
            id: "gone".into(),
            name: None,
        },
    ];
    let people = [
        Person {
            id: "pat".into(),
            label: "Pat".into(),
        },
        Person {
            id: "sam".into(),
            label: "Sam".into(),
        },
    ];
    let retired = ["gone".to_owned()];
    let roster = Roster::new(&members, &people, &retired);
    let desks = [Desk {
        id: "eng".into(),
        name: "Engineering".into(),
        description: None,
        members: vec!["ada".into(), "ben".into()],
        responder_mode: ResponderMode::Lead,
    }];
    let set = DeskSet::new(&desks, &[], &[], &[], &[]);
    let edit = request(
        "ada",
        "eng",
        "edit",
        resource("/repo/src/lib.rs"),
        Effect::Mutating,
    );
    let read = request(
        "ada",
        "eng",
        "read",
        resource("/repo/src/lib.rs"),
        Effect::ReadOnly,
    );
    let now = Millis(1_000);
    let ask = |policy: &ApprovalPolicy,
               request: &ApprovalRequest,
               grants: &[StandingGrant],
               refusals: &[RememberedRefusal]| {
        show(&approve(
            request, policy, grants, refusals, &roster, &set, now,
        ))
    };

    let on = policy();
    let repo = TargetPattern::Resource {
        root: "/repo".into(),
    };
    println!("  -- policy");
    println!(
        "  {:<44} {}",
        "defaults (Ask, person pat)",
        ask(&on, &edit, &[], &[])
    );
    println!(
        "  {:<44} {}",
        "enabled=false",
        ask(
            &ApprovalPolicy {
                enabled: false,
                ..policy()
            },
            &edit,
            &[],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "default=Deny, no rules",
        ask(
            &ApprovalPolicy {
                default: DefaultVerdict::Deny,
                ..policy()
            },
            &edit,
            &[],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "rule: read-only -> Allow (a read)",
        ask(
            &ApprovalPolicy {
                rules: vec![rule(Some(Effect::ReadOnly), None, None, RuleVerdict::Allow)],
                ..policy()
            },
            &read,
            &[],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "rule: read-only -> Allow (an edit)",
        ask(
            &ApprovalPolicy {
                rules: vec![rule(Some(Effect::ReadOnly), None, None, RuleVerdict::Allow)],
                ..policy()
            },
            &edit,
            &[],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "rule: verb edit under /repo -> Deny",
        ask(
            &ApprovalPolicy {
                rules: vec![rule(
                    None,
                    Some("edit"),
                    Some(repo.clone()),
                    RuleVerdict::Deny
                )],
                ..policy()
            },
            &edit,
            &[],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "Deny beats a matching Allow",
        ask(
            &ApprovalPolicy {
                rules: vec![
                    rule(None, None, None, RuleVerdict::Allow),
                    rule(None, Some("edit"), None, RuleVerdict::Deny)
                ],
                ..policy()
            },
            &edit,
            &[],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "rule: Ask under Deny default",
        ask(
            &ApprovalPolicy {
                default: DefaultVerdict::Deny,
                rules: vec![rule(None, Some("edit"), None, RuleVerdict::Ask)],
                ..policy()
            },
            &edit,
            &[],
            &[]
        )
    );
    let named = request(
        "ada",
        "eng",
        "send",
        ActionTarget::Named {
            name: "deploy".into(),
        },
        Effect::Mutating,
    );
    println!(
        "  {:<44} {}",
        "a Resource pattern never matches a Named target",
        ask(
            &ApprovalPolicy {
                rules: vec![rule(None, None, Some(repo.clone()), RuleVerdict::Allow)],
                ..policy()
            },
            &named,
            &[],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "Named pattern matches by name",
        ask(
            &ApprovalPolicy {
                rules: vec![rule(
                    None,
                    None,
                    Some(TargetPattern::Named {
                        name: "deploy".into()
                    }),
                    RuleVerdict::Allow
                )],
                ..policy()
            },
            &named,
            &[],
            &[]
        )
    );

    println!("  -- who is asked");
    println!(
        "  {:<44} {}",
        "approver Absent",
        ask(
            &ApprovalPolicy {
                approver: ApproverRule::Absent,
                ..policy()
            },
            &edit,
            &[],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "approver names a person not in the roster",
        ask(
            &ApprovalPolicy {
                approver: ApproverRule::Person {
                    id: "nobody".into()
                },
                ..policy()
            },
            &edit,
            &[],
            &[]
        )
    );
    let per_desk = ApproverRule::PerDesk {
        default: "pat".into(),
        overrides: vec![DeskApprover {
            desk_id: "eng".into(),
            person_id: "sam".into(),
        }],
    };
    println!(
        "  {:<44} {}",
        "PerDesk, override for eng",
        ask(
            &ApprovalPolicy {
                approver: per_desk.clone(),
                ..policy()
            },
            &edit,
            &[],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "PerDesk, a desk with no override",
        ask(
            &ApprovalPolicy {
                approver: ApproverRule::PerDesk {
                    default: "pat".into(),
                    overrides: vec![]
                },
                ..policy()
            },
            &edit,
            &[],
            &[]
        )
    );
    let elsewhere = request(
        "ada",
        "nowhere",
        "edit",
        resource("/repo/src/lib.rs"),
        Effect::Mutating,
    );
    println!(
        "  {:<44} {}",
        "PerDesk, the desk cannot be resolved",
        ask(
            &ApprovalPolicy {
                approver: per_desk,
                ..policy()
            },
            &elsewhere,
            &[],
            &[]
        )
    );

    println!("  -- the request itself");
    println!(
        "  {:<44} {}",
        "actor retired",
        ask(
            &on,
            &request("gone", "eng", "edit", resource("/repo/a"), Effect::Mutating),
            &[],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "effect Unclassified",
        ask(
            &on,
            &request(
                "ada",
                "eng",
                "edit",
                resource("/repo/a"),
                Effect::Unclassified
            ),
            &[],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "path escapes with `..`",
        ask(
            &on,
            &request(
                "ada",
                "eng",
                "edit",
                resource("/repo/../etc/passwd"),
                Effect::Mutating
            ),
            &[],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "blank verb",
        ask(
            &on,
            &request("ada", "eng", " ", resource("/repo/a"), Effect::Mutating),
            &[],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "blank call id",
        ask(
            &on,
            &ApprovalRequest {
                call_id: " ".into(),
                ..edit.clone()
            },
            &[],
            &[]
        )
    );

    println!("  -- standing grants and remembered refusals");
    let call = grant(GrantScope::Call, &edit, 500, None);
    let action = grant(GrantScope::Action, &edit, 500, None);
    let tree = grant(
        GrantScope::Resource {
            root: "/repo".into(),
        },
        &edit,
        500,
        None,
    );
    let other_call = ApprovalRequest {
        call_id: "call-2".into(),
        ..edit.clone()
    };
    let sibling = request(
        "ada",
        "eng",
        "edit",
        resource("/repo/src/main.rs"),
        Effect::Mutating,
    );
    let outside = request(
        "ada",
        "eng",
        "edit",
        resource("/etc/hosts"),
        Effect::Mutating,
    );
    println!(
        "  {:<44} {}",
        "Call grant, same call",
        ask(&on, &edit, std::slice::from_ref(&call), &[])
    );
    println!(
        "  {:<44} {}",
        "Call grant, another call",
        ask(&on, &other_call, std::slice::from_ref(&call), &[])
    );
    println!(
        "  {:<44} {}",
        "Action grant, another call",
        ask(&on, &other_call, std::slice::from_ref(&action), &[])
    );
    println!(
        "  {:<44} {}",
        "Resource grant, a sibling file",
        ask(&on, &sibling, std::slice::from_ref(&tree), &[])
    );
    println!(
        "  {:<44} {}",
        "Resource grant, outside the root",
        ask(&on, &outside, std::slice::from_ref(&tree), &[])
    );
    println!(
        "  {:<44} {}",
        "narrowest grant wins (Call over Resource)",
        ask(&on, &edit, &[tree.clone(), call.clone()], &[])
    );
    println!(
        "  {:<44} {}",
        "allow_grants=false",
        ask(
            &ApprovalPolicy {
                allow_grants: false,
                ..policy()
            },
            &edit,
            std::slice::from_ref(&call),
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "grant revoked",
        ask(
            &on,
            &edit,
            &[StandingGrant {
                revoked: true,
                ..call.clone()
            }],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "grant not yet granted (now < granted_at)",
        ask(
            &on,
            &edit,
            &[grant(GrantScope::Call, &edit, 5_000, None)],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "grant expired",
        ask(
            &on,
            &edit,
            &[grant(GrantScope::Call, &edit, 500, Some(900))],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "grant from a later epoch",
        ask(
            &on,
            &edit,
            &[StandingGrant {
                granted_at_epoch: ConsentEpoch(9),
                ..call.clone()
            }],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "grant at a later sequence, same epoch",
        ask(
            &on,
            &edit,
            &[StandingGrant {
                granted_at_epoch: ConsentEpoch(2),
                granted_at_sequence: 99,
                ..call.clone()
            }],
            &[]
        )
    );
    let capped = ApprovalPolicy {
        max_grant_ttl: Some(Millis(100)),
        ..policy()
    };
    println!(
        "  {:<44} {}",
        "max_grant_ttl=100, grant never expires",
        ask(&capped, &edit, std::slice::from_ref(&call), &[])
    );
    println!(
        "  {:<44} {}",
        "max_grant_ttl=100, ttl 400",
        ask(
            &capped,
            &edit,
            &[grant(GrantScope::Call, &edit, 500, Some(1_500))],
            &[]
        )
    );
    println!(
        "  {:<44} {}",
        "max_grant_ttl=100, ttl 100",
        ask(
            &capped,
            &edit,
            &[grant(GrantScope::Call, &edit, 950, Some(1_050))],
            &[]
        )
    );
    let refusal = RememberedRefusal {
        key: ScopeKey::for_request(&edit),
        scope: GrantScope::Action,
        epoch: ConsentEpoch(2),
    };
    println!(
        "  {:<44} {}",
        "remembered refusal beats a grant",
        ask(
            &on,
            &edit,
            std::slice::from_ref(&call),
            std::slice::from_ref(&refusal)
        )
    );
    println!(
        "  {:<44} {}",
        "remembered refusal from an older epoch",
        ask(
            &on,
            &edit,
            &[],
            &[RememberedRefusal {
                epoch: ConsentEpoch(1),
                ..refusal
            }]
        )
    );
    println!(
        "  ScopeKey::render = {}",
        ScopeKey::for_request(&edit).render()
    );
    println!(
        "  GrantScope::covers: Call vs another call = {}; Action vs another call = {}; Resource vs sibling = {}",
        GrantScope::Call.covers(
            &ScopeKey::for_request(&edit),
            &ScopeKey::for_request(&other_call)
        ),
        GrantScope::Action.covers(
            &ScopeKey::for_request(&edit),
            &ScopeKey::for_request(&other_call)
        ),
        GrantScope::Resource {
            root: "/repo".into()
        }
        .covers(
            &ScopeKey::for_request(&edit),
            &ScopeKey::for_request(&sibling)
        )
    );
    Ok(())
}
