//! Total, pure approval for one side-effecting action.

#[cfg(test)]
mod test;

mod types;

pub use types::{
    Action, ActionTarget, AllowBasis, ApprovalDecision, ApprovalPolicy, ApprovalRequest,
    ApprovalRule, ApproverRule, ConsentEpoch, DefaultVerdict, DenyReason, DeskApprover, Effect,
    GrantScope, Millis, RememberedRefusal, RuleVerdict, ScopeKey, StandingGrant, TargetPattern,
};

use crate::{desk::DeskSet, roster::Roster};

/// Decide whether an action may run, must be refused, or needs one person.
///
/// This function is total: malformed state is a denial, never an error a host
/// could accidentally propagate past the gate. It performs no IO and executes
/// nothing.
#[must_use]
pub fn approve(
    request: &ApprovalRequest,
    policy: &ApprovalPolicy,
    grants: &[StandingGrant],
    refusals: &[RememberedRefusal],
    roster: &Roster<'_>,
    desks: &DeskSet<'_>,
    now: Millis,
) -> ApprovalDecision {
    if !policy.enabled {
        return denied(DenyReason::Disabled);
    }
    if malformed(request, roster, desks) {
        return denied(DenyReason::MalformedRequest);
    }
    if roster.active_member(&request.actor_id).is_none() {
        return denied(DenyReason::UnknownActor);
    }
    if request.action.effect == Effect::Unclassified {
        return denied(DenyReason::UnclassifiedAction);
    }

    let matching: Vec<&ApprovalRule> = policy
        .rules
        .iter()
        .filter(|rule| rule_matches(rule, &request.action))
        .collect();
    if matching
        .iter()
        .any(|rule| rule.verdict == RuleVerdict::Deny)
    {
        return denied(DenyReason::PolicyDenied);
    }

    let key = ScopeKey::for_request(request);
    if refusals.iter().any(|refusal| {
        refusal.epoch == request.epoch && scope_covers(&refusal.scope, &refusal.key, &key)
    }) {
        return denied(DenyReason::RememberedRefusal);
    }

    if policy.allow_grants
        && let Some(grant) = best_grant(request, policy, grants, &key, now)
    {
        return ApprovalDecision::Allow {
            basis: AllowBasis::Grant {
                key: grant.key.clone(),
            },
        };
    }
    if matching
        .iter()
        .any(|rule| rule.verdict == RuleVerdict::Allow)
    {
        return ApprovalDecision::Allow {
            basis: AllowBasis::Policy,
        };
    }
    if matching.iter().any(|rule| rule.verdict == RuleVerdict::Ask)
        || policy.default == DefaultVerdict::Ask
    {
        return ask(request, policy, roster, desks, key);
    }
    denied(DenyReason::NoRule)
}

fn malformed(request: &ApprovalRequest, roster: &Roster<'_>, desks: &DeskSet<'_>) -> bool {
    request.actor_id.trim().is_empty()
        || request.call_id.trim().is_empty()
        || request.action.verb.trim().is_empty()
        || request.action.target.value().trim().is_empty()
        || has_parent_component(request.action.target.value())
        || roster.validate().is_err()
        || desks.validate().is_err()
}

fn denied(reason: DenyReason) -> ApprovalDecision {
    ApprovalDecision::Deny { reason }
}

fn rule_matches(rule: &ApprovalRule, action: &Action) -> bool {
    rule.effect.is_none_or(|effect| effect == action.effect)
        && rule.verb.as_ref().is_none_or(|verb| verb == &action.verb)
        && rule
            .target
            .as_ref()
            .is_none_or(|pattern| target_matches(pattern, &action.target))
}

fn target_matches(pattern: &TargetPattern, target: &ActionTarget) -> bool {
    match (pattern, target) {
        (TargetPattern::Named { name }, ActionTarget::Named { name: target }) => name == target,
        (TargetPattern::Resource { root }, ActionTarget::Resource { path }) => {
            path_within(path, root)
        }
        _ => false,
    }
}

fn grant_live(grant: &StandingGrant, policy: &ApprovalPolicy, now: Millis) -> bool {
    if grant.revoked || now < grant.granted_at || grant.expires_at.is_some_and(|at| now >= at) {
        return false;
    }
    match (grant.expires_at, policy.max_grant_ttl) {
        (None, Some(_)) => false,
        (Some(expires), Some(cap)) => expires
            .0
            .checked_sub(grant.granted_at.0)
            .is_some_and(|ttl| ttl <= cap.0),
        _ => true,
    }
}

fn best_grant<'a>(
    request: &ApprovalRequest,
    policy: &ApprovalPolicy,
    grants: &'a [StandingGrant],
    key: &ScopeKey,
    now: Millis,
) -> Option<&'a StandingGrant> {
    grants
        .iter()
        .filter(|grant| {
            grant_live(grant, policy, now)
                && (grant.granted_at_epoch, grant.granted_at_sequence)
                    <= (request.epoch, request.sequence)
                && scope_covers(&grant.scope, &grant.key, key)
        })
        .min_by_key(|grant| {
            (
                scope_rank(&grant.scope),
                grant.granted_at_epoch,
                grant.granted_at_sequence,
                grant.key.render(),
            )
        })
}

const fn scope_rank(scope: &GrantScope) -> u8 {
    match scope {
        GrantScope::Call => 0,
        GrantScope::Action => 1,
        GrantScope::Resource { .. } => 2,
    }
}

fn scope_covers(scope: &GrantScope, held: &ScopeKey, requested: &ScopeKey) -> bool {
    if held.actor_id != requested.actor_id || held.verb != requested.verb {
        return false;
    }
    match scope {
        GrantScope::Call => held.call_id == requested.call_id && held.target == requested.target,
        GrantScope::Action => held.target == requested.target,
        GrantScope::Resource { root } => matches!(
            &requested.target,
            ActionTarget::Resource { path } if path_within(path, root)
        ),
    }
}

fn ask(
    request: &ApprovalRequest,
    policy: &ApprovalPolicy,
    roster: &Roster<'_>,
    desks: &DeskSet<'_>,
    key: ScopeKey,
) -> ApprovalDecision {
    let person_id = match &policy.approver {
        ApproverRule::Person { id } => id.as_str(),
        ApproverRule::PerDesk { default, overrides } => {
            let Ok(desk_id) = desks.resolve_id(&request.conversation.desk_id) else {
                return denied(DenyReason::UnresolvableApprover);
            };
            overrides
                .iter()
                .find(|override_| override_.desk_id == desk_id)
                .map_or(default.as_str(), |override_| override_.person_id.as_str())
        }
    };
    if roster.person(person_id).is_none() {
        return denied(DenyReason::NoApprover);
    }
    ApprovalDecision::Ask {
        who: person_id.to_owned(),
        scope: GrantScope::Call,
        key,
        epoch: request.epoch,
    }
}

fn has_parent_component(path: &str) -> bool {
    path.split('/').any(|component| component == "..")
}

fn path_within(path: &str, root: &str) -> bool {
    if path.is_empty()
        || root.is_empty()
        || has_parent_component(path)
        || has_parent_component(root)
        || path.starts_with('/') != root.starts_with('/')
    {
        return false;
    }
    let path: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let root: Vec<&str> = root.split('/').filter(|part| !part.is_empty()).collect();
    path.starts_with(&root)
}
