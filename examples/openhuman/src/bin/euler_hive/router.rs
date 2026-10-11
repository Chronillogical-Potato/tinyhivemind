//! A deterministic `Router` for desk broadcasts: the colleague a broadcast
//! names is the colleague it reaches.
//!
//! `hive_broadcast` takes an explicit `to`, and the host commits the message
//! as `@<to> <message>`. This router reads that mention and answers core's
//! routing question with certainty for that candidate; a broadcast that names
//! no eligible colleague gets no evaluation, so core's driver falls back to
//! its own deterministic responder. Core still validates the evaluation's
//! domain and owns the plan; this only replaces a semantic model with the
//! seat's stated intent.

use tinyhivemind_core::embed::{
    CandidateProbability, ContributionProbability, EvaluationDisposition, Router, RouterFuture,
    RoutingEvaluation, RoutingRequest,
};
use tinyhivemind_core::runtime::responder::{PROBABILITY_SCALE, Probability};

/// Routes a broadcast to the colleague its leading `@mention` names.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct MentionRouter;

/// The candidate a message's first `@<id>` names, if it is one.
pub(super) fn addressed(request: &RoutingRequest) -> Option<&str> {
    let message = request.message.trim_start();
    let named = message.strip_prefix('@')?;
    let id: String = named
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    request
        .candidates
        .iter()
        .find(|candidate| candidate.available && candidate.id == id)
        .map(|candidate| candidate.id.as_str())
}

fn certain(id: &str, request: &RoutingRequest) -> Option<RoutingEvaluation> {
    let full = Probability::new(PROBABILITY_SCALE)?;
    let zero = Probability::new(0)?;
    let eligible: Vec<&str> = request
        .candidates
        .iter()
        .filter(|candidate| candidate.available)
        .map(|candidate| candidate.id.as_str())
        .collect();
    let mut primary: Vec<CandidateProbability> = eligible
        .iter()
        .map(|candidate| CandidateProbability {
            candidate_id: (*candidate).to_owned(),
            probability: if *candidate == id { full } else { zero },
        })
        .collect();
    primary.push(CandidateProbability {
        candidate_id: "none".into(),
        probability: zero,
    });
    Some(RoutingEvaluation {
        primary_responder: id.to_owned(),
        primary_probabilities: primary,
        confidence: full,
        needs_collaboration: zero,
        needs_clarification: zero,
        contributions: eligible
            .iter()
            .map(|candidate| ContributionProbability {
                candidate_id: (*candidate).to_owned(),
                probability: zero,
            })
            .collect(),
        high_impact: zero,
        model_identity: "euler-hive/mention".into(),
        question_schema_version: 1,
        roster_version: request.roster_version,
        disposition: EvaluationDisposition::Unchecked,
    })
}

/// Why the router declined to evaluate.
#[derive(Debug)]
struct NoColleagueNamed;

impl std::fmt::Display for NoColleagueNamed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the broadcast names no eligible colleague")
    }
}

impl std::error::Error for NoColleagueNamed {}

impl Router for MentionRouter {
    fn evaluate<'a>(&'a self, request: &'a RoutingRequest) -> RouterFuture<'a> {
        Box::pin(async move {
            addressed(request)
                .and_then(|id| certain(id, request))
                .ok_or_else(|| Box::new(NoColleagueNamed) as tinyhivemind_core::embed::RouterError)
        })
    }
}
