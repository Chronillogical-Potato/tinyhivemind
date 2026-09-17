//! One-call runtime edge from the total approval fold to a host-owned human gate.

#[cfg(test)]
mod test;

mod types;

pub use types::{ApprovalAnswer, ApprovalOutcome, ApprovalPrompt, AskOutcome};

use std::{future::Future, pin::Pin};

use crate::{BoxError, Result};
pub use tinyhivemind_core::approval::*;

/// Boxed executor-neutral future returned by [`ApprovalGate`].
pub type ApprovalFuture<'a> =
    Pin<Box<dyn Future<Output = std::result::Result<AskOutcome, BoxError>> + Send + 'a>>;

/// Host-owned atomic boundary for asking one person at most once.
pub trait ApprovalGate: Send + Sync {
    /// Revalidate the committed request and policy, durably create at most one
    /// prompt under `prompt.dedupe_key`, and return any recorded answer.
    fn ask_once(&self, prompt: ApprovalPrompt) -> ApprovalFuture<'_>;
}

/// Apply a pure approval decision and ask the host at most once.
///
/// Allow and deny decisions call the gate zero times. An ask calls it exactly
/// once and validates that any persisted grant or refusal did not widen the
/// offered scope.
///
/// # Errors
///
/// Returns [`crate::Error::ApprovalGate`] for an unexpected host failure or
/// [`crate::Error::InvalidApprovalAnswer`] for a widened/mismatched answer.
pub async fn request_approval(
    gate: &(dyn ApprovalGate + '_),
    request: &ApprovalRequest,
    decision: ApprovalDecision,
) -> Result<ApprovalOutcome> {
    let (who, scope, key, epoch) = match decision {
        ApprovalDecision::Allow { basis } => return Ok(ApprovalOutcome::Allowed { basis }),
        ApprovalDecision::Deny { reason } => return Ok(ApprovalOutcome::Denied { reason }),
        ApprovalDecision::Ask {
            who,
            scope,
            key,
            epoch,
        } => (who, scope, key, epoch),
    };
    let prompt = ApprovalPrompt {
        who,
        scope: scope.clone(),
        key: key.clone(),
        epoch,
        request_sequence: request.sequence,
        dedupe_key: dedupe_key(&key, request.sequence),
    };
    let outcome = gate
        .ask_once(prompt)
        .await
        .map_err(|source| crate::Error::ApprovalGate { source })?;
    match outcome {
        AskOutcome::Asked => Ok(ApprovalOutcome::Asked),
        AskOutcome::Already => Ok(ApprovalOutcome::Already),
        AskOutcome::Answered { answer } => answer_outcome(answer, &scope, &key, epoch),
    }
}

fn answer_outcome(
    answer: ApprovalAnswer,
    offered_scope: &GrantScope,
    key: &ScopeKey,
    epoch: ConsentEpoch,
) -> Result<ApprovalOutcome> {
    match answer {
        ApprovalAnswer::Approved { grant } => {
            if grant.as_ref().is_some_and(|grant| {
                grant.scope != *offered_scope
                    || grant.key != *key
                    || grant.granted_at_epoch != epoch
            }) {
                return Err(crate::Error::InvalidApprovalAnswer);
            }
            Ok(ApprovalOutcome::Approved { grant })
        }
        ApprovalAnswer::Refused { refusal } => {
            if refusal.as_ref().is_some_and(|refusal| {
                refusal.scope != *offered_scope || refusal.key != *key || refusal.epoch != epoch
            }) {
                return Err(crate::Error::InvalidApprovalAnswer);
            }
            Ok(ApprovalOutcome::Refused { refusal })
        }
    }
}

fn dedupe_key(key: &ScopeKey, sequence: u64) -> String {
    let sequence = sequence.to_string();
    format!("{}s{}:{sequence}", key.render(), sequence.len())
}
