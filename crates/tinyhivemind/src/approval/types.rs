//! Stable prompt, host answer, and final runtime outcome payloads.

use serde::{Deserialize, Serialize};
use tinyhivemind_core::approval::{
    AllowBasis, ConsentEpoch, DenyReason, GrantScope, RememberedRefusal, ScopeKey, StandingGrant,
};

/// Exact one-person question handed to the host approval UI.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalPrompt {
    /// Exact person id resolved by policy.
    pub who: String,
    /// Maximum grant scope the answer may claim.
    pub scope: GrantScope,
    /// Exact action scope.
    pub key: ScopeKey,
    /// Consent epoch in which the question applies.
    pub epoch: ConsentEpoch,
    /// Sequence of the request being gated.
    pub request_sequence: u64,
    /// Collision-free key for atomic ask-once behavior.
    pub dedupe_key: String,
}

/// A person's recorded answer to an approval prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum ApprovalAnswer {
    /// The person approved, optionally minting the exact offered grant.
    Approved {
        /// Standing grant, or no persistent grant for this one execution.
        grant: Option<StandingGrant>,
    },
    /// The person refused, optionally remembering it for this epoch.
    Refused {
        /// Epoch-scoped refusal, or no remembered refusal.
        refusal: Option<RememberedRefusal>,
    },
}

/// Result returned by the host's atomic ask-once transaction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AskOutcome {
    /// A new pending approval was durably created.
    Asked,
    /// The dedupe key already has a pending record.
    ///
    /// A completed record must be returned as [`Self::Answered`] so the
    /// caller observes the durable answer.
    Already,
    /// A final answer was already available.
    Answered {
        /// Recorded answer.
        answer: ApprovalAnswer,
    },
}

/// Final runtime result of applying a pure decision and optional host wait.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ApprovalOutcome {
    /// Pure policy or a standing grant already allowed the action.
    Allowed {
        /// Deterministic allow basis.
        basis: AllowBasis,
    },
    /// Pure policy denied the action.
    Denied {
        /// Closed denial reason.
        reason: DenyReason,
    },
    /// A new human question is pending.
    Asked,
    /// The exact question was already recorded.
    Already,
    /// The person approved the action.
    Approved {
        /// Optional standing grant the host recorded.
        grant: Option<StandingGrant>,
    },
    /// The person refused the action.
    Refused {
        /// Optional epoch-scoped refusal the host recorded.
        refusal: Option<RememberedRefusal>,
    },
}
