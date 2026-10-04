//! The router acceptance rules: policy thresholds against a scripted router.

use tinyhivemind_core::embed::{
    ConversationKind, RouteCandidate, Router, RoutingPlan, RoutingPolicy, RoutingRequest,
    RoutingSource, route_broadcast, route_message,
};
use tinyhivemind_core::runtime::responder::Probability;
use tinyhivemind_lab::{KeywordRouter, Res, block_on, section};

use crate::fixture::{hive, routing_policy};

fn plan(plan: &RoutingPlan) -> String {
    match plan {
        RoutingPlan::One {
            responder_id,
            evaluation,
        } => {
            format!(
                "One({responder_id}) conf {:.2}",
                f64::from(evaluation.confidence.parts()) / 1e6
            )
        }
        RoutingPlan::Hive {
            primary_id,
            invited_ids,
            ..
        } => format!("Hive({primary_id} + {invited_ids:?})"),
        RoutingPlan::Clarify { .. } => "Clarify".into(),
        RoutingPlan::Fallback {
            responder_id,
            reason,
        } => format!("Fallback({responder_id}, {reason:?})"),
    }
}

fn p(parts: u32) -> Probability {
    Probability::new(parts).unwrap_or(Probability::ZERO)
}

struct Row<'a> {
    label: &'a str,
    message: &'a str,
    policy: RoutingPolicy,
    primary: Option<&'a KeywordRouter>,
    reasoning: Option<&'a KeywordRouter>,
}

pub fn run() -> Res {
    section("embed routing: RoutingPolicy against a scripted router");
    let hive = hive();
    let base = routing_policy(3);
    let sure = KeywordRouter::new("small");
    let unsure = KeywordRouter {
        confidence: Some(300_000),
        ..KeywordRouter::new("small-unsure")
    };
    let strong = KeywordRouter::new("large");
    let stale = KeywordRouter {
        roster_skew: 1,
        ..KeywordRouter::new("small-stale")
    };
    let down = KeywordRouter {
        fail: true,
        ..KeywordRouter::new("small-down")
    };
    let vague = KeywordRouter {
        needs_clarification: 800_000,
        ..KeywordRouter::new("small-vague")
    };
    let nothing = KeywordRouter {
        none_weight: 5_000,
        ..KeywordRouter::new("small-none")
    };
    let risky = KeywordRouter {
        high_impact: 900_000,
        confidence: Some(700_000),
        ..KeywordRouter::new("small-risky")
    };
    let rows = [
        Row {
            label: "confident, one seat fits",
            message: "fix the rust parser",
            policy: base.clone(),
            primary: Some(&sure),
            reasoning: None,
        },
        Row {
            label: "two seats fit, policy width 3",
            message: "write rust parser tests",
            policy: base.clone(),
            primary: Some(&sure),
            reasoning: None,
        },
        Row {
            label: "two seats fit, policy width 1",
            message: "write rust parser tests",
            policy: routing_policy(1),
            primary: Some(&sure),
            reasoning: None,
        },
        Row {
            label: "unsure, no reasoning router",
            message: "fix the rust parser",
            policy: base.clone(),
            primary: Some(&unsure),
            reasoning: None,
        },
        Row {
            label: "unsure, escalates to a strong router",
            message: "fix the rust parser",
            policy: base.clone(),
            primary: Some(&unsure),
            reasoning: Some(&strong),
        },
        Row {
            label: "unsure, reasoning also unsure",
            message: "fix the rust parser",
            policy: base.clone(),
            primary: Some(&unsure),
            reasoning: Some(&unsure),
        },
        Row {
            label: "minimum_confidence 0.99",
            message: "fix the rust parser",
            policy: RoutingPolicy {
                minimum_confidence: p(990_000),
                ..base.clone()
            },
            primary: Some(&sure),
            reasoning: None,
        },
        Row {
            label: "high impact, confidence 0.7 < 0.8",
            message: "fix the rust parser",
            policy: base.clone(),
            primary: Some(&risky),
            reasoning: None,
        },
        Row {
            label: "high impact, minimum relaxed to 0.6",
            message: "fix the rust parser",
            policy: RoutingPolicy {
                high_impact_minimum_confidence: p(600_000),
                ..base.clone()
            },
            primary: Some(&risky),
            reasoning: None,
        },
        Row {
            label: "needs clarification 0.8 >= 0.6",
            message: "fix the rust parser",
            policy: base.clone(),
            primary: Some(&vague),
            reasoning: None,
        },
        Row {
            label: "clarification_threshold 0.95",
            message: "fix the rust parser",
            policy: RoutingPolicy {
                clarification_threshold: p(950_000),
                ..base.clone()
            },
            primary: Some(&vague),
            reasoning: None,
        },
        Row {
            label: "router answers none",
            message: "unrelated gardening",
            policy: base.clone(),
            primary: Some(&nothing),
            reasoning: None,
        },
        Row {
            label: "stale roster version",
            message: "fix the rust parser",
            policy: base.clone(),
            primary: Some(&stale),
            reasoning: None,
        },
        Row {
            label: "router unavailable",
            message: "fix the rust parser",
            policy: base.clone(),
            primary: Some(&down),
            reasoning: None,
        },
        Row {
            label: "no router supplied",
            message: "fix the rust parser",
            policy: base.clone(),
            primary: None,
            reasoning: None,
        },
        Row {
            label: "choice_option_limit 1 (invalid)",
            message: "fix the rust parser",
            policy: RoutingPolicy {
                choice_option_limit: 1,
                ..base.clone()
            },
            primary: Some(&sure),
            reasoning: None,
        },
        Row {
            label: "round_width 0 (invalid)",
            message: "fix the rust parser",
            policy: routing_policy(0),
            primary: Some(&sure),
            reasoning: None,
        },
    ];
    println!("  {:<40} {:<46} calls(primary/reasoning)", "case", "plan");
    for row in rows {
        let request = hive.desk_request(row.message, Vec::new(), None, 1, row.policy.clone());
        let (a, b) = (
            row.primary.map_or(0, KeywordRouter::calls),
            row.reasoning.map_or(0, KeywordRouter::calls),
        );
        let routed = block_on(hive.route_desk(
            row.primary.map(|r| r as &dyn Router),
            row.reasoning.map(|r| r as &dyn Router),
            &request,
            None,
            "planner",
        ))?;
        let (a, b) = (
            row.primary.map_or(0, KeywordRouter::calls) - a,
            row.reasoning.map_or(0, KeywordRouter::calls) - b,
        );
        println!("  {:<40} {:<46} {a}/{b}", row.label, plan(&routed));
    }

    section("route_message: bypasses before any router is asked");
    let request = hive.desk_request("fix the rust parser", Vec::new(), None, 1, base.clone());
    let router = KeywordRouter::new("small");
    let asked = |request: &RoutingRequest, explicit: Option<&str>| {
        let before = router.calls();
        let routed = block_on(route_message(
            Some(&router),
            None,
            request,
            explicit,
            "planner",
        ));
        format!(
            "{} (router calls: {})",
            plan(&routed),
            router.calls() - before
        )
    };
    println!(
        "  explicit mention of writer: {}",
        asked(&request, Some("writer"))
    );
    for kind in [
        ConversationKind::Direct,
        ConversationKind::General,
        ConversationKind::Workflow,
        ConversationKind::Desk,
    ] {
        let mut shaped = request.clone();
        shaped.conversation.kind = kind;
        println!(
            "  conversation kind {kind:?} (may_open_hive={}): {}",
            shaped.conversation.may_open_hive(),
            asked(&shaped, None)
        );
    }
    let mut empty = request.clone();
    empty.candidates = request
        .candidates
        .iter()
        .cloned()
        .map(|c| RouteCandidate {
            available: false,
            ..c
        })
        .collect();
    println!("  every candidate unavailable: {}", asked(&empty, None));
    let mut from_agent = request.clone();
    from_agent.source = RoutingSource::AgentBroadcast {
        author_id: "planner".into(),
    };
    println!(
        "  an agent broadcast sent through route_message: {}",
        asked(&from_agent, None)
    );

    section("route_broadcast: the author may not be a candidate");
    let mut broadcast = request.clone();
    broadcast.source = RoutingSource::AgentBroadcast {
        author_id: "planner".into(),
    };
    broadcast.candidates.retain(|c| c.id != "planner");
    let before = router.calls();
    println!(
        "  planner broadcasts, planner excluded: {} (calls {})",
        plan(&block_on(route_broadcast(
            Some(&router),
            None,
            &broadcast,
            "writer"
        ))),
        router.calls() - before
    );
    let mut included = request.clone();
    included.source = RoutingSource::AgentBroadcast {
        author_id: "planner".into(),
    };
    println!(
        "  planner still in the candidate list: {}",
        plan(&block_on(route_broadcast(
            Some(&router),
            None,
            &included,
            "writer"
        )))
    );
    println!(
        "  desk-message source: {}",
        plan(&block_on(route_broadcast(
            Some(&router),
            None,
            &request,
            "writer"
        )))
    );
    println!(
        "  CONCURRENT_CHOICE_THRESHOLD_PARTS = {}",
        tinyhivemind_core::embed::CONCURRENT_CHOICE_THRESHOLD_PARTS
    );
    let mismatched = RoutingRequest {
        desk_purpose: Some("something else".into()),
        ..request.clone()
    };
    println!(
        "  desk request that no longer matches the graph: {}",
        block_on(hive.route_desk(Some(&router), None, &mismatched, None, "planner"))
            .err()
            .map_or("ok".into(), |e| e.to_string())
    );
    println!(
        "  unknown fallback responder: {}",
        block_on(hive.route_desk(Some(&router), None, &request, None, "ghost"))
            .err()
            .map_or("ok".into(), |e| e.to_string())
    );
    Ok(())
}
