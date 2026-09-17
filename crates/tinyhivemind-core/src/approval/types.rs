//! Stable approval request, policy, grant, and decision payloads.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::dispatch::DispatchConversation;

/// Host-supplied monotonic millisecond reading.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Millis(pub u64);

/// Host-owned consent epoch advanced when a person gives new direction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ConsentEpoch(pub u64);

/// One side-effecting action awaiting authorization.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalRequest {
    /// Consent epoch in which the request was minted.
    pub epoch: ConsentEpoch,
    /// Sequence drawn from the same host ordering as grant sequences.
    pub sequence: u64,
    /// Host call id.
    pub call_id: String,
    /// Authenticated active agent requesting the action.
    pub actor_id: String,
    /// Conversation in which the action arose.
    pub conversation: DispatchConversation,
    /// Exact action descriptor.
    pub action: Action,
}

/// A typed action descriptor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Action {
    /// Host-defined operation verb.
    pub verb: String,
    /// Named or resource target.
    pub target: ActionTarget,
    /// Host-declared effect classification.
    pub effect: Effect,
}

/// What an action addresses.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionTarget {
    /// Opaque named target.
    Named {
        /// Target name.
        name: String,
    },
    /// Caller-normalized lexical resource path.
    Resource {
        /// Resource path.
        path: String,
    },
}

impl ActionTarget {
    pub(super) fn value(&self) -> &str {
        match self {
            Self::Named { name } => name,
            Self::Resource { path } => path,
        }
    }
}

/// Host-declared side-effect class.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Reads state without changing it.
    ReadOnly,
    /// May change external or durable state.
    Mutating,
    /// The host cannot classify the effect; approval denies.
    Unclassified,
}

/// Collision-free fields identifying one approval scope.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ScopeKey {
    /// Acting agent.
    pub actor_id: String,
    /// Host call id.
    pub call_id: String,
    /// Action verb.
    pub verb: String,
    /// Action target.
    pub target: ActionTarget,
}

impl ScopeKey {
    /// Build the scope key for a request.
    #[must_use]
    pub fn for_request(request: &ApprovalRequest) -> Self {
        Self {
            actor_id: request.actor_id.clone(),
            call_id: request.call_id.clone(),
            verb: request.action.verb.clone(),
            target: request.action.target.clone(),
        }
    }

    /// Render a collision-free printable host deduplication token.
    #[must_use]
    pub fn render(&self) -> String {
        let (tag, target) = match &self.target {
            ActionTarget::Named { name } => ("n", name.as_str()),
            ActionTarget::Resource { path } => ("r", path.as_str()),
        };
        [
            field("a", &self.actor_id),
            field("c", &self.call_id),
            field("v", &self.verb),
            field(tag, target),
        ]
        .concat()
    }
}

fn field(tag: &str, value: &str) -> String {
    format!("{tag}{}:{value}", value.len())
}

/// How far a standing grant reaches.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GrantScope {
    /// This exact call id only.
    Call,
    /// The same actor, verb, and target across calls.
    Action,
    /// The same actor and verb at or below one lexical resource root.
    Resource {
        /// Caller-normalized lexical root.
        root: String,
    },
}

/// Previously issued standing authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct StandingGrant {
    /// Coverage declared by the approver.
    pub scope: GrantScope,
    /// Action identity from the originating question.
    pub key: ScopeKey,
    /// Epoch in which the grant was issued.
    pub granted_at_epoch: ConsentEpoch,
    /// Sequence in the host's shared ordering.
    pub granted_at_sequence: u64,
    /// Time the grant started.
    pub granted_at: Millis,
    /// Exclusive expiration, or no expiration.
    pub expires_at: Option<Millis>,
    /// Explicit revocation marker.
    pub revoked: bool,
}

/// A refusal remembered only inside one consent epoch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RememberedRefusal {
    /// Refused action identity.
    pub key: ScopeKey,
    /// Coverage of the refusal.
    pub scope: GrantScope,
    /// Epoch in which it applies.
    pub epoch: ConsentEpoch,
}

/// Total approval policy supplied by the host.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalPolicy {
    /// Kill switch; false denies rather than bypasses.
    pub enabled: bool,
    /// Fallback when no rule decides.
    pub default: DefaultVerdict,
    /// Order-independent policy rules.
    pub rules: Vec<ApprovalRule>,
    /// Human approver resolution.
    pub approver: ApproverRule,
    /// Whether standing grants may allow.
    pub allow_grants: bool,
    /// Maximum accepted lifetime for a grant.
    pub max_grant_ttl: Option<Millis>,
}

/// Fail-closed policy default.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultVerdict {
    /// Deny when no rule decides.
    Deny,
    /// Ask the configured person when no rule decides.
    Ask,
}

/// One typed policy rule.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalRule {
    /// Optional effect match.
    pub effect: Option<Effect>,
    /// Optional exact verb match.
    pub verb: Option<String>,
    /// Optional exact or contained target match.
    pub target: Option<TargetPattern>,
    /// Decision contributed by a matching rule.
    pub verdict: RuleVerdict,
}

/// Target predicate used by a policy rule.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TargetPattern {
    /// Exact named target.
    Named {
        /// Required target name.
        name: String,
    },
    /// Resource root containing the target path.
    Resource {
        /// Required lexical root.
        root: String,
    },
}

/// Rule result. Deny wins regardless of rule order.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleVerdict {
    /// Refuse the action.
    Deny,
    /// Ask the configured person.
    Ask,
    /// Permit the action when no denial applies.
    Allow,
}

/// How to resolve the one human approver.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApproverRule {
    /// One person for every request.
    Person {
        /// Exact person id.
        id: String,
    },
    /// Default person with canonical desk-specific overrides.
    PerDesk {
        /// Default person id.
        default: String,
        /// Desk-specific replacements.
        overrides: Vec<DeskApprover>,
    },
}

/// One canonical desk-to-person approver override.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DeskApprover {
    /// Canonical desk id.
    pub desk_id: String,
    /// Exact person id.
    pub person_id: String,
}

/// Total outcome of the approval fold.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum ApprovalDecision {
    /// The action may proceed under a deterministic basis.
    Allow {
        /// Why it was allowed.
        basis: AllowBasis,
    },
    /// The action must not proceed.
    Deny {
        /// Closed operator-facing reason.
        reason: DenyReason,
    },
    /// Exactly one person must answer before the action proceeds.
    Ask {
        /// Exact person id.
        who: String,
        /// Maximum grant scope the answer may mint.
        scope: GrantScope,
        /// Exact request scope.
        key: ScopeKey,
        /// Epoch in which the answer applies.
        epoch: ConsentEpoch,
    },
}

/// Why an action was allowed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AllowBasis {
    /// A matching policy rule allowed it.
    Policy,
    /// A live standing grant covered it.
    Grant {
        /// Exact originating grant key.
        key: ScopeKey,
    },
}

/// Closed reasons an approval gate denies.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DenyReason {
    /// Policy kill switch is off.
    Disabled,
    /// Request or snapshots are structurally malformed.
    MalformedRequest,
    /// Authenticated actor is not active.
    UnknownActor,
    /// Host did not classify the action effect.
    UnclassifiedAction,
    /// A matching deny rule won.
    PolicyDenied,
    /// A same-epoch remembered refusal covers the request.
    RememberedRefusal,
    /// The desk used to choose an approver did not resolve uniquely.
    UnresolvableApprover,
    /// The configured id did not name a person.
    NoApprover,
    /// No rule allowed or requested approval.
    NoRule,
}

impl fmt::Display for DenyReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Disabled => "approval is turned off, so this action cannot run",
            Self::MalformedRequest => "the action request is malformed and cannot run",
            Self::UnknownActor => "the acting agent is not available, so this action cannot run",
            Self::UnclassifiedAction => "the action effect is unknown and cannot run",
            Self::PolicyDenied => "current policy does not allow this action",
            Self::RememberedRefusal => "this action was already refused under current direction",
            Self::NoRule => "no approval rule allows this action",
            Self::UnresolvableApprover | Self::NoApprover => {
                "there is no available person to approve this action"
            }
        })
    }
}
