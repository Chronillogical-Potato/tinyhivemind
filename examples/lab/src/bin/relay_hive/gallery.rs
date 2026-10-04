//! Every reason a referral is refused, one input each.

use tinyhivemind_core::dispatch::{DispatchConversation, DispatchKey};
use tinyhivemind_core::mention::{MentionAuthor, resolve};
use tinyhivemind_core::referral::{
    ReferralDecision, ReferralInput, ReferralOrigin, ReferralPolicy, ReferralReach, referral,
};
use tinyhivemind_lab::{Res, section};

use crate::world;

pub fn run() -> Res {
    section("referral: the reason gallery");
    let world = world();
    let roster = world.roster();
    let desks = world.desks();
    let open = ReferralPolicy {
        enabled: true,
        max_hops: 2,
        reach: ReferralReach::Desks,
        returns: true,
    };
    let input = |author: &str, desk: &str, said: &str, hop: u32, origin: Option<ReferralOrigin>| {
        ReferralInput {
            key: DispatchKey {
                trigger_sequence: 1,
            },
            conversation: DispatchConversation {
                desk_id: desk.into(),
                thread_root: None,
            },
            author_id: author.into(),
            content: said.into(),
            mentions: resolve(
                said,
                None,
                &MentionAuthor::Agent { id: author.into() },
                &roster,
                &desks,
            ),
            hop,
            origin,
        }
    };
    let back = Some(ReferralOrigin {
        conversation: DispatchConversation {
            desk_id: "support".into(),
            thread_root: None,
        },
        asker_id: "alice".into(),
    });
    let cases: Vec<(&str, ReferralPolicy, ReferralInput)> = vec![
        (
            "Disabled",
            ReferralPolicy::DEFAULT,
            input("alice", "support", "@carol hi", 0, None),
        ),
        (
            "HopLimitReached",
            open,
            input("alice", "support", "@carol hi", 2, None),
        ),
        (
            "SourceInactive",
            open,
            input("gus", "ghosts", "@carol hi", 0, None),
        ),
        (
            "NoReferralTarget (nobody named, no origin)",
            open,
            input("alice", "support", "just a note", 0, None),
        ),
        (
            "SelfMention",
            open,
            input("alice", "support", "@alice hi", 0, None),
        ),
        (
            "TargetInactive",
            open,
            input("alice", "support", "@gus hi", 0, None),
        ),
        (
            "SelfDesk",
            open,
            input("alice", "support", "@support hi", 0, None),
        ),
        (
            "EmptyDesk (only a retired member)",
            open,
            input("alice", "support", "@ghosts hi", 0, None),
        ),
        (
            "TargetDeskless",
            open,
            input("alice", "support", "@hal hi", 0, None),
        ),
        (
            "UnknownDesk (a desk mention needs a known desk)",
            open,
            input("alice", "support", "@nowhere hi", 0, None),
        ),
        (
            "HopOverflow",
            ReferralPolicy {
                max_hops: u32::MAX,
                ..open
            },
            input("alice", "support", "@carol hi", u32::MAX, None),
        ),
        (
            "Local reach keeps the turn in place",
            ReferralPolicy {
                reach: ReferralReach::Local,
                ..open
            },
            input("alice", "support", "@carol hi", 0, None),
        ),
        (
            "Channels reach does not address desks",
            ReferralPolicy {
                reach: ReferralReach::Channels,
                ..open
            },
            input("alice", "support", "@backend hi", 0, None),
        ),
        (
            "a return needs an origin on another desk",
            open,
            input("carol", "backend", "done", 0, back.clone()),
        ),
        (
            "a reply on the origin's own desk is already home",
            open,
            input("alice", "support", "done", 0, back),
        ),
    ];
    for (label, policy, input) in cases {
        let shown = match referral(policy, &input, &roster, &desks) {
            Ok(ReferralDecision::None { reason }) => format!("None({reason:?}): {reason}"),
            Ok(ReferralDecision::One { referral }) => format!(
                "One({:?} {} -> {} on {}, crosses={}, origin={})",
                referral.kind,
                referral.source_id,
                referral.target_id,
                referral.to.desk_id,
                referral.crosses(),
                referral.origin.is_some()
            ),
            Err(error) => format!("error: {error}"),
        };
        println!("  {label:<50} {shown}");
    }
    Ok(())
}
