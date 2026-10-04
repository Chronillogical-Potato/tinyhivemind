//! A keyword router: the model-free stand-in for a semantic `Router`.
//!
//! A real router asks a model for a probability distribution over the eligible
//! candidates. This one counts word overlap between the message and each
//! candidate's capabilities, then normalizes, so the same message always gets
//! the same distribution and the examples stay deterministic. Its knobs force
//! the uncertain, stale and failing answers the acceptance rules exist for.

use std::sync::atomic::{AtomicUsize, Ordering};

use tinyhivemind_core::embed::{
    CandidateProbability, ContributionProbability, EvaluationDisposition, RouteCandidate, Router,
    RouterFuture, RoutingEvaluation, RoutingRequest,
};
use tinyhivemind_core::runtime::responder::{PROBABILITY_SCALE, Probability};

/// How the scripted router behaves; the knobs are builder methods.
#[derive(Debug)]
pub struct KeywordRouter {
    /// Model identity stamped on every evaluation.
    pub model: &'static str,
    /// Weight of the `none` answer against each candidate's `100 + 400 * hits`.
    none_weight: u32,
    /// Report this confidence instead of the primary's probability.
    confidence: Option<u32>,
    /// Report this need for clarification, in parts per million.
    needs_clarification: u32,
    /// Report this high-impact probability, in parts per million.
    high_impact: u32,
    /// Added to the request's roster version, so the answer is stale.
    roster_skew: u64,
    /// Fail every call.
    fail: bool,
    calls: AtomicUsize,
}

impl KeywordRouter {
    /// A router that is sure of itself and never fails.
    #[must_use]
    pub fn new(model: &'static str) -> Self {
        Self {
            model,
            none_weight: 50,
            confidence: None,
            needs_clarification: 0,
            high_impact: 0,
            roster_skew: 0,
            fail: false,
            calls: AtomicUsize::new(0),
        }
    }

    /// Report this confidence instead of the primary's probability.
    #[must_use]
    pub fn confidence(mut self, parts: u32) -> Self {
        self.confidence = Some(parts);
        self
    }

    /// Report this need for clarification.
    #[must_use]
    pub fn clarification(mut self, parts: u32) -> Self {
        self.needs_clarification = parts;
        self
    }

    /// Report this high-impact probability.
    #[must_use]
    pub fn high_impact(mut self, parts: u32) -> Self {
        self.high_impact = parts;
        self
    }

    /// Weigh the `none` answer this heavily.
    #[must_use]
    pub fn none_weight(mut self, weight: u32) -> Self {
        self.none_weight = weight;
        self
    }

    /// Answer for a roster version this far ahead of the request's.
    #[must_use]
    pub fn roster_skew(mut self, skew: u64) -> Self {
        self.roster_skew = skew;
        self
    }

    /// Fail every call.
    #[must_use]
    pub fn failing(mut self) -> Self {
        self.fail = true;
        self
    }

    /// How many times the router has been asked.
    #[must_use]
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }

    fn hits(request: &RoutingRequest, candidate: &RouteCandidate) -> usize {
        let words: Vec<String> = request
            .message
            .split(|c: char| !c.is_alphanumeric())
            .map(str::to_lowercase)
            .collect();
        candidate
            .capabilities
            .iter()
            .chain(&candidate.learned_topics)
            .filter(|topic| words.contains(&topic.to_lowercase()))
            .count()
    }

    fn score(&self, request: &RoutingRequest) -> RoutingEvaluation {
        let eligible: Vec<&RouteCandidate> =
            request.candidates.iter().filter(|c| c.available).collect();
        let mut weights: Vec<(String, u64)> = eligible
            .iter()
            .map(|c| (c.id.clone(), 100 + 400 * Self::hits(request, c) as u64))
            .collect();
        weights.push(("none".into(), u64::from(self.none_weight)));
        let total: u64 = weights.iter().map(|(_, w)| w).sum();
        let mut parts: Vec<(String, u32)> = weights
            .iter()
            .map(|(id, w)| {
                (
                    id.clone(),
                    u32::try_from(w * u64::from(PROBABILITY_SCALE) / total).unwrap_or(0),
                )
            })
            .collect();
        let best = parts
            .iter()
            .enumerate()
            .max_by_key(|(index, (_, p))| (*p, std::cmp::Reverse(*index)))
            .map_or(0, |(index, _)| index);
        let assigned: u32 = parts.iter().map(|(_, p)| p).sum();
        parts[best].1 += PROBABILITY_SCALE - assigned;
        let primary = parts[best].clone();
        let p = |parts: u32| Probability::new(parts).unwrap_or(Probability::ZERO);
        RoutingEvaluation {
            primary_responder: primary.0,
            primary_probabilities: parts
                .iter()
                .map(|(id, parts)| CandidateProbability {
                    candidate_id: id.clone(),
                    probability: p(*parts),
                })
                .collect(),
            confidence: p(self.confidence.unwrap_or(primary.1)),
            needs_collaboration: p(0),
            needs_clarification: p(self.needs_clarification),
            contributions: eligible
                .iter()
                .map(|c| ContributionProbability {
                    candidate_id: c.id.clone(),
                    probability: p(if Self::hits(request, c) > 0 {
                        600_000
                    } else {
                        20_000
                    }),
                })
                .collect(),
            high_impact: p(self.high_impact),
            model_identity: self.model.into(),
            question_schema_version: 1,
            roster_version: request.roster_version + self.roster_skew,
            disposition: EvaluationDisposition::Unchecked,
        }
    }
}

impl Router for KeywordRouter {
    fn evaluate<'a>(&'a self, request: &'a RoutingRequest) -> RouterFuture<'a> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let result = if self.fail {
            Err("the router is down".into())
        } else {
            Ok(self.score(request))
        };
        Box::pin(async move { result })
    }
}
