//! One bug, relayed support -> backend -> infra and back, under each policy.

use std::collections::BTreeMap;

use tinyhivemind_core::dispatch::{DispatchConversation, DispatchKey};
use tinyhivemind_core::embed::{
    ConversationKind, ConversationRef, RouteCandidate, RoutingPlan, RoutingRequest, RoutingSource,
    route_message,
};
use tinyhivemind_core::mention::{MentionAuthor, resolve};
use tinyhivemind_core::referral::{
    ReferralDecision, ReferralInput, ReferralKind, ReferralOrigin, ReferralPolicy, ReferralReach,
    referral,
};
use tinyhivemind_core::runtime::SessionAuthor;
use tinyhivemind_core::telemetry::{TraceEvent, Tracer};
use tinyhivemind_lab::{KeywordRouter, MemoryLog, Res, TraceRig, agent, block_on, section};

use crate::world;

const BUG: &str = "checkout times out for EU users";

/// What a desk makes of a message it is handed: its findings, and whether it
/// needs another desk. Rules, not a model.
fn findings(desk: &str, handed: &str) -> (String, bool) {
    let answered = handed.to_lowercase().contains("renewed");
    match desk {
        "support" if answered => (format!("Customer update: {handed}"), false),
        "support" => (format!("Triaged: {handed}."), true),
        "backend" if answered => (
            "Confirmed with infra: the certificate is renewed and checkout is healthy.".into(),
            false,
        ),
        "backend" => (
            "The api is fine, but the tls handshake to eu-gw fails; the certificate looks wrong."
                .into(),
            true,
        ),
        _ => (
            "Found it: the certificate for eu-gw expired. Renewed.".into(),
            false,
        ),
    }
}

/// Pick the next desk for `text` from every desk but `current`, as a router
/// over desk-shaped candidates would.
fn pick_desk(router: &KeywordRouter, current: &str, text: &str) -> Option<String> {
    let table: [(&str, &[&str]); 3] = [
        ("support", &["customer", "refund", "triage"]),
        ("backend", &["checkout", "api", "retries"]),
        ("infra", &["tls", "certificate", "network"]),
    ];
    let candidates: Vec<RouteCandidate> = table
        .iter()
        .filter(|(id, _)| *id != current)
        .map(|(id, caps)| RouteCandidate {
            id: (*id).into(),
            label: (*id).into(),
            role: None,
            description: None,
            capabilities: caps.iter().map(|c| (*c).to_owned()).collect(),
            learned_topics: Vec::new(),
            available: true,
        })
        .collect();
    let request = RoutingRequest {
        message: text.into(),
        source: RoutingSource::DeskMessage,
        conversation: ConversationRef {
            id: current.into(),
            kind: ConversationKind::Desk,
            thread_root: None,
        },
        desk_purpose: None,
        thread_context: Vec::new(),
        candidates,
        roster_version: 1,
        policy: crate::relay::policy(),
    };
    match block_on(route_message(Some(router), None, &request, None, current)) {
        RoutingPlan::One { responder_id, .. } if responder_id != current => Some(responder_id),
        _ => None,
    }
}

fn policy() -> tinyhivemind_core::embed::RoutingPolicy {
    use tinyhivemind_core::runtime::responder::Probability;
    let p = |n| Probability::new(n).unwrap_or(Probability::ZERO);
    tinyhivemind_core::embed::RoutingPolicy {
        minimum_confidence: p(400_000),
        high_impact_minimum_confidence: p(800_000),
        clarification_threshold: p(900_000),
        high_impact_threshold: p(900_000),
        round_width: 1,
        choice_option_limit: 8,
    }
}

/// Where a relay ended.
#[derive(Debug)]
pub struct Relay {
    pub path: Vec<String>,
    pub hops: u32,
    pub returned: bool,
    pub stopped: String,
}

/// Run the relay under `policy`, narrating each hop to the log and tracer.
pub fn relay(policy: ReferralPolicy, tracer: &Tracer<'_>, narrate: bool) -> Relay {
    let world = world();
    let roster = world.roster();
    let desks = world.desks();
    let router = KeywordRouter::new("desk-router");
    let mut log = MemoryLog::default();
    let mut outcome = Relay {
        path: vec!["support/alice".into()],
        hops: 0,
        returned: false,
        stopped: String::new(),
    };

    // Each conversation remembers who asked it, because a return carries no
    // origin of its own: the host keeps the stack.
    let mut asked_by: BTreeMap<String, ReferralOrigin> = BTreeMap::new();
    let mut desk = "support".to_owned();
    let mut seat = "alice".to_owned();
    let mut handed = BUG.to_owned();
    let mut hop = 0_u32;
    log.say(&desk, SessionAuthor::Operator, BUG);
    for turn in 1..=12_u64 {
        tracer.emit(TraceEvent::TurnStarted {
            turn,
            seat: seat.clone(),
        });
        let (found, needs_help) = findings(&desk, &handed);
        let routed = needs_help
            .then(|| pick_desk(&router, &desk, &found))
            .flatten();
        let said = match routed
            .as_deref()
            .and_then(|to| desks.lead(to).ok().flatten().map(|lead| (to, lead)))
        {
            // The desk by name, and its lead by handle, so every reach has
            // something it is allowed to act on.
            Some((to, lead)) => format!("{found} @{to} please take this (@{lead})."),
            None => found,
        };
        let sequence = log.say(&desk, agent(&seat), &said);
        tracer.emit(TraceEvent::TurnFinished {
            turn,
            seat: seat.clone(),
            input_tokens: handed.len() as u64 / 4,
            output_tokens: said.len() as u64 / 4,
            latency_ms: 0,
        });
        if narrate {
            println!("  ^{sequence:<2} [{desk}] @{seat}: {said}");
        }
        let input = ReferralInput {
            key: DispatchKey {
                trigger_sequence: sequence.0,
            },
            conversation: DispatchConversation {
                desk_id: desk.clone(),
                thread_root: None,
            },
            author_id: seat.clone(),
            content: said.clone(),
            mentions: resolve(
                &said,
                None,
                &MentionAuthor::Agent { id: seat.clone() },
                &roster,
                &desks,
            ),
            hop,
            origin: asked_by.get(&desk).cloned(),
        };
        let decision = match referral(policy, &input, &roster, &desks) {
            Ok(decision) => decision,
            Err(error) => {
                outcome.stopped = format!("error: {error}");
                return outcome;
            }
        };
        match decision {
            ReferralDecision::None { reason } => {
                tracer.emit(TraceEvent::Mark {
                    label: "referral.none".into(),
                    detail: format!("{desk}/{seat}: {reason:?}"),
                });
                outcome.stopped = format!("{reason:?} at {desk}/{seat}");
                outcome.returned = desk == "support" && hop > 0 && seat == "alice";
                return outcome;
            }
            ReferralDecision::One { referral } => {
                tracer.emit(TraceEvent::Mark {
                    label: format!("referral.{:?}", referral.kind).to_lowercase(),
                    detail: format!(
                        "{} -> {} on {} (hop {}, crosses {})",
                        referral.source_id,
                        referral.target_id,
                        referral.to.desk_id,
                        referral.child_hop,
                        referral.crosses()
                    ),
                });
                if narrate {
                    println!(
                        "       {:?} to @{} on {} (hop {}, crosses={})",
                        referral.kind,
                        referral.target_id,
                        referral.to.desk_id,
                        referral.child_hop,
                        referral.crosses()
                    );
                }
                if let (ReferralKind::Forward, Some(origin)) =
                    (referral.kind, referral.origin.clone())
                {
                    asked_by.insert(referral.to.desk_id.clone(), origin);
                }
                hop = referral.child_hop;
                outcome.hops = hop;
                desk = referral.to.desk_id.clone();
                seat = referral.target_id.clone();
                handed = referral.content.clone();
                outcome.path.push(format!("{desk}/{seat}"));
            }
        }
    }
    outcome.stopped = "step cap".into();
    outcome
}

pub fn run(rig: &TraceRig) -> Res {
    section("relay: support -> backend -> infra and the answer back");
    let open = ReferralPolicy {
        enabled: true,
        max_hops: 6,
        reach: ReferralReach::Desks,
        returns: true,
    };
    let tracer = rig.tracer("relay:narrated");
    let story = relay(open, &tracer, true);
    println!(
        "  -> path {:?}, {} hops, returned to the origin: {}, stopped: {}",
        story.path, story.hops, story.returned, story.stopped
    );

    section("ReferralPolicy swept: enabled, reach, returns, max_hops");
    println!(
        "  {:<44} {:>4}  {:<9} stopped",
        "policy", "hops", "returned"
    );
    let mut rows: Vec<(String, ReferralPolicy)> =
        vec![("DEFAULT (disabled)".into(), ReferralPolicy::DEFAULT)];
    for reach in [
        ReferralReach::Local,
        ReferralReach::Channels,
        ReferralReach::Desks,
    ] {
        rows.push((
            format!("reach={reach:?}, returns, max_hops=6"),
            ReferralPolicy { reach, ..open },
        ));
    }
    rows.push((
        "reach=Desks, returns=false".into(),
        ReferralPolicy {
            returns: false,
            ..open
        },
    ));
    for hops in [1, 2, 3, 4, 5] {
        rows.push((
            format!("reach=Desks, returns, max_hops={hops}"),
            ReferralPolicy {
                max_hops: hops,
                ..open
            },
        ));
    }
    for (label, policy) in rows {
        let tracer = rig.tracer(&format!("relay:{label}"));
        let r = relay(policy, &tracer, false);
        println!(
            "  {label:<44} {:>4}  {:<9} {}",
            r.hops, r.returned, r.stopped
        );
    }
    println!(
        "  ReferralReach: Local.crosses={} Channels.crosses={} Desks.addresses_desks={}",
        ReferralReach::Local.crosses(),
        ReferralReach::Channels.crosses(),
        ReferralReach::Desks.addresses_desks()
    );
    Ok(())
}
