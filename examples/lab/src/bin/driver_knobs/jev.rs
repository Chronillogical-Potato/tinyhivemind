//! `JevRouter` over a scripted System One transport: what a route costs.
//!
//! The transport answers Choice and Noul questions by keyword overlap, counts
//! the calls and the bytes it was sent, and can be told to fail or to answer
//! with something the schema forbids.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;
use tinyhivemind_core::embed::{RouteCandidate, RoutingPlan, route_message};
use tinyhivemind_core::typesafe::{
    ChoiceAnswer, Error, JevRouter, NoulAnswer, Question, RetryClass, SystemOneAnswer,
    SystemOneRequest, SystemOneResponse, SystemOneTransport, SystemOneTransportFuture, TokenUsage,
    classify_retry,
};
use tinyhivemind_lab::{Res, block_on, section};

use crate::fixture::{candidates, hive, routing_policy};

/// How the transport misbehaves.
#[derive(Clone, Copy)]
pub enum Fault {
    None,
    Down(u16),
    NoChoice,
    SumsToTwo,
}

pub struct Scripted {
    fault: Fault,
    calls: AtomicUsize,
    questions: AtomicUsize,
    bytes: AtomicUsize,
}

impl Scripted {
    pub fn new(fault: Fault) -> Self {
        Self {
            fault,
            calls: AtomicUsize::new(0),
            questions: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
        }
    }

    fn tally(&self) -> (usize, usize, usize) {
        (
            self.calls.load(Ordering::Relaxed),
            self.questions.load(Ordering::Relaxed),
            self.bytes.load(Ordering::Relaxed),
        )
    }

    fn hits(request: &SystemOneRequest, capabilities: &[String]) -> usize {
        let message = request.state["message"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase();
        let words: Vec<&str> = message.split(|c: char| !c.is_alphanumeric()).collect();
        capabilities
            .iter()
            .filter(|c| words.contains(&c.as_str()))
            .count()
    }

    fn answer(&self, request: &SystemOneRequest) -> SystemOneResponse {
        let mut answers = BTreeMap::new();
        for (key, question) in &request.questions {
            let answer = match question {
                Question::Choice { criteria, .. } => {
                    let mut weights: BTreeMap<String, f64> = BTreeMap::new();
                    for (id, detail) in criteria {
                        let caps: Vec<String> = detail
                            .as_ref()
                            .and_then(|d| d["capabilities"].as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(Value::as_str)
                                    .map(str::to_owned)
                                    .collect()
                            })
                            .unwrap_or_default();
                        weights.insert(
                            id.clone(),
                            if id == "none" {
                                0.05
                            } else {
                                0.1 + 0.4 * Self::hits(request, &caps) as f64
                            },
                        );
                    }
                    let total: f64 = weights.values().sum();
                    let scale = if matches!(self.fault, Fault::SumsToTwo) {
                        2.0
                    } else {
                        1.0
                    };
                    let probabilities: BTreeMap<String, f64> = weights
                        .iter()
                        .map(|(k, v)| (k.clone(), v / total * scale))
                        .collect();
                    let choice = probabilities
                        .iter()
                        .max_by(|a, b| a.1.total_cmp(b.1).then_with(|| b.0.cmp(a.0)))
                        .map(|(k, _)| k.clone())
                        .unwrap_or_default();
                    let confidence = probabilities.get(&choice).copied().unwrap_or(0.0).min(1.0);
                    SystemOneAnswer::Choice(ChoiceAnswer {
                        choice,
                        probabilities,
                        confidence,
                    })
                }
                Question::Noul { .. } => {
                    let noul = if let Some(id) = key.strip_prefix("contributes_") {
                        let detail = request.state["candidates"]
                            .as_array()
                            .and_then(|all| all.iter().find(|c| c["id"] == id));
                        let caps: Vec<String> = detail
                            .and_then(|d| d["capabilities"].as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(Value::as_str)
                                    .map(str::to_owned)
                                    .collect()
                            })
                            .unwrap_or_default();
                        if Self::hits(request, &caps) > 0 {
                            0.6
                        } else {
                            0.05
                        }
                    } else {
                        0.05
                    };
                    SystemOneAnswer::Noul(NoulAnswer { noul })
                }
            };
            answers.insert(key.clone(), answer);
        }
        if matches!(self.fault, Fault::NoChoice) {
            answers.remove("primary_responder");
        }
        SystemOneResponse {
            model: "scripted-jev".into(),
            answers,
            usage: TokenUsage {
                input_tokens: 0,
                output_tokens: 0,
            },
        }
    }
}

impl SystemOneTransport for Scripted {
    fn evaluate<'a>(&'a self, request: &'a SystemOneRequest) -> SystemOneTransportFuture<'a> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.questions
            .fetch_add(request.questions.len(), Ordering::Relaxed);
        self.bytes.fetch_add(
            serde_json::to_string(request).map_or(0, |s| s.len()),
            Ordering::Relaxed,
        );
        let result = match self.fault {
            Fault::Down(status) => Err(Error::Transport {
                status: Some(status),
                message: "provider says no".into(),
            }),
            _ => Ok(self.answer(request)),
        };
        Box::pin(async move { result })
    }
}

fn many(n: usize) -> Vec<RouteCandidate> {
    (1..=n)
        .map(|i| RouteCandidate {
            id: format!("seat{i}"),
            label: format!("Seat {i}"),
            role: None,
            description: None,
            capabilities: vec![format!("topic{i}"), "shared".into()],
            learned_topics: Vec::new(),
            available: true,
        })
        .collect()
}

pub fn run() -> Res {
    section("JevRouter over a scripted System One transport");
    let hive = hive();
    println!("  {:<44} {:<40} calls questions bytes", "case", "plan");
    let cases: Vec<(&str, Fault, usize, Vec<RouteCandidate>, &str)> = vec![
        (
            "4 candidates, option limit 8",
            Fault::None,
            8,
            candidates(),
            "fix the rust parser",
        ),
        (
            "4 candidates, option limit 4 (== count)",
            Fault::None,
            4,
            candidates(),
            "fix the rust parser",
        ),
        (
            "4 candidates, option limit 3",
            Fault::None,
            3,
            candidates(),
            "fix the rust parser",
        ),
        (
            "12 candidates, option limit 16",
            Fault::None,
            16,
            many(12),
            "needs topic7 work",
        ),
        (
            "12 candidates, option limit 5",
            Fault::None,
            5,
            many(12),
            "needs topic7 work",
        ),
        (
            "24 candidates, option limit 5",
            Fault::None,
            5,
            many(24),
            "needs topic19 work",
        ),
        (
            "transport down: 503",
            Fault::Down(503),
            8,
            candidates(),
            "fix the rust parser",
        ),
        (
            "transport rate limited: 429",
            Fault::Down(429),
            8,
            candidates(),
            "fix the rust parser",
        ),
        (
            "response omits the Choice",
            Fault::NoChoice,
            8,
            candidates(),
            "fix the rust parser",
        ),
        (
            "Choice distribution sums to 2",
            Fault::SumsToTwo,
            8,
            candidates(),
            "fix the rust parser",
        ),
        (
            "option limit 1 (no room for none)",
            Fault::None,
            1,
            candidates(),
            "fix the rust parser",
        ),
    ];
    for (label, fault, limit, candidates, message) in cases {
        let transport = Scripted::new(fault);
        let router = JevRouter::new(transport);
        let mut request = hive.desk_request(message, Vec::new(), None, 1, routing_policy(1));
        request.candidates = candidates;
        request.policy.choice_option_limit = limit;
        let plan = block_on(route_message(
            Some(&router),
            None,
            &request,
            None,
            "planner",
        ));
        let shown = match &plan {
            RoutingPlan::One { responder_id, .. } => format!("One({responder_id})"),
            RoutingPlan::Hive { primary_id, .. } => format!("Hive({primary_id}+)"),
            RoutingPlan::Clarify { .. } => "Clarify".into(),
            RoutingPlan::Fallback { reason, .. } => format!("Fallback({reason:?})"),
        };
        let (calls, questions, bytes) = router.transport().tally();
        println!("  {label:<44} {shown:<40} {calls:>5} {questions:>9} {bytes:>5}");
    }
    let model = JevRouter::with_model(Scripted::new(Fault::None), "jev-large");
    let mut request = hive.desk_request(
        "fix the rust parser",
        Vec::new(),
        None,
        1,
        routing_policy(1),
    );
    if let RoutingPlan::One { evaluation, .. } =
        block_on(route_message(Some(&model), None, &request, None, "planner"))
    {
        println!(
            "  with_model(\"jev-large\") -> model_identity={:?} schema_version={}",
            evaluation.model_identity, evaluation.question_schema_version
        );
    }
    request.candidates[0].id = "none".into();
    let bad = JevRouter::new(Scripted::new(Fault::None));
    println!(
        "  a candidate named `none`: {:?}",
        block_on(tinyhivemind_core::embed::Router::evaluate(&bad, &request))
            .err()
            .map(|e| e.to_string())
    );
    for status in [400, 401, 429, 500, 529] {
        println!(
            "  classify_retry({status}) = {}",
            if classify_retry(status) == RetryClass::Retryable {
                "Retryable"
            } else {
                "Permanent"
            }
        );
    }
    println!("  route_message never retries: a 429 became Fallback(ProviderUnavailable) above.");
    Ok(())
}
