//! The model-backed rung of the responder ladder, and asking it who should
//! open the desk.
//!
//! [`LadderSelector`] is the harness's [`Selector`]: given the desk's
//! candidates and the operator's message, it asks the same endpoint the
//! seats run on to name one id. [`route_opening`] is the call site — it
//! resolves the opening instruction's mentions and hands the ladder to
//! `choose_responder`, which validates the answer and falls back
//! deterministically when it is not a candidate.

use crate::agent::{self, Backend};
use crate::cli::Options;
use crate::host::OPERATOR_ID;

use tinyhivemind::mention::{MentionAuthor, resolve as resolve_mentions};
use tinyhivemind::responder::{
    CandidateProbability, Probability, ResponderRequest, SelectionEvaluation, SelectionPolicy,
    Selector, SelectorCandidate, SelectorFuture, accept_selection, choose_responder,
};

/// One model-backed rung of the responder ladder.
///
/// This is the same endpoint the seats run on, asked a different and much
/// smaller question: given the desk's candidates and the operator's message,
/// name one id. The library validates the answer through `accept_selection`
/// and falls back deterministically when it is not a candidate, so a wrong
/// answer here costs a rung rather than the run.
pub(crate) struct LadderSelector {
    /// The backend asked to name a responder. Only the `Http` variant is
    /// ever wired to a selector; see the `Debug` impl below.
    pub(crate) backend: Backend,
}

impl std::fmt::Debug for LadderSelector {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LadderSelector")
            .field("backend", &self.backend)
            .finish()
    }
}

impl Selector for LadderSelector {
    fn select<'a>(
        &'a self,
        request: &'a tinyhivemind::responder::SelectionRequest,
    ) -> SelectorFuture<'a> {
        Box::pin(async move {
            let candidates = request
                .candidates
                .iter()
                .map(|candidate| {
                    format!(
                        "{} — {}{}",
                        candidate.id,
                        candidate.role,
                        candidate
                            .description
                            .as_ref()
                            .map(|text| format!(" ({text})"))
                            .unwrap_or_default(),
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            let system = "You route one message to one member of a team. \
                Reply with exactly one member id and nothing else."
                .to_owned();
            let user = format!(
                "Team members:\n{candidates}\n\nMessage:\n{}\n\nWhich one id should answer?",
                request.message,
            );
            let reply = match &self.backend {
                Backend::Http {
                    base,
                    key,
                    model,
                    timeout_secs,
                    thinking,
                } => agent::http_turn(base, key, model, *timeout_secs, *thinking, &system, &user),
                // A CLI seat is driven per turn; the selector reuses the same
                // last mile so a `--agent-cmd` run still exercises this rung.
                Backend::Command { .. } => Err("no selector on a CLI backend".to_owned()),
            };
            let reply = reply.map_err(|message| -> tinyhivemind::BoxError { message.into() })?;
            let choice = accept_selection(&reply, &request.candidates).ok_or_else(
                || -> tinyhivemind::BoxError { "selector named no candidate".into() },
            )?;
            Ok(SelectionEvaluation {
                probabilities: request
                    .candidates
                    .iter()
                    .map(|candidate| CandidateProbability {
                        candidate_id: candidate.id.clone(),
                        probability: if candidate.id == choice {
                            Probability::ONE
                        } else {
                            Probability::ZERO
                        },
                    })
                    .collect(),
                choice,
                confidence: Probability::ONE,
            })
        })
    }
}

/// Ask the responder ladder who should answer an unaddressed instruction.
///
/// This is the first of the two edges the harness exists to exercise. The
/// desk runs in `ResponderMode::Auto`, so with more than one effective member
/// the pure plan asks for a selection rather than deciding, and the model
/// rung actually runs. A selector that fails or names a non-candidate costs
/// the rung and not the run: the library falls back deterministically and
/// records why in the disposition.
pub(crate) async fn route_opening(
    options: &Options,
    selector: Option<&(dyn Selector + '_)>,
    seats: &[(&str, &str)],
    roster: &tinyhivemind_core::roster::Roster<'_>,
    desks: &tinyhivemind_core::desk::DeskSet<'_>,
) -> Result<tinyhivemind::responder::ResponderDecision, String> {
    let opening_mentions = resolve_mentions(
        &options.instruction,
        None,
        &MentionAuthor::Person {
            id: OPERATOR_ID.to_owned(),
        },
        roster,
        desks,
    );
    let candidates: Vec<SelectorCandidate> = seats
        .iter()
        .map(|(id, role)| SelectorCandidate {
            id: (*id).to_owned(),
            label: (*id).to_owned(),
            role: (*role).to_owned(),
            description: None,
        })
        .collect();
    choose_responder(
        selector,
        &ResponderRequest {
            message: options.instruction.clone(),
            chat: Some(crate::host::DESK_ID.to_owned()),
            mentions: opening_mentions,
            orchestrator_id: "planner".to_owned(),
            selection_policy: SelectionPolicy::Allowed,
            minimum_selection_confidence: Probability::ZERO,
        },
        roster,
        desks,
        &candidates,
    )
    .await
    .map_err(|error| format!("the responder ladder failed: {error}"))
}
