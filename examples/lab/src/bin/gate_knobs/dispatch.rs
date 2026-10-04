//! `mention_dispatch`: every reason one mentioned turn is refused.

use tinyhivemind_core::dispatch::{
    DispatchConversation, DispatchKey, MentionDispatchDecision, MentionDispatchInput,
    MentionDispatchPolicy, NO_AVAILABLE_TARGET, mention_dispatch,
};
use tinyhivemind_core::mention::{Mention, MentionTarget};
use tinyhivemind_lab::{Res, World, section};

pub fn run() -> Res {
    section("mention_dispatch: the reason gallery (hand-built mentions)");
    let world = World::new()
        .agent("alice")
        .agent("bob")
        .agent("gus")
        .desk("eng", "Engineering", "Build", &["alice", "bob", "gus"])
        .retire("gus");
    let roster = world.roster();
    let on = MentionDispatchPolicy {
        enabled: true,
        max_hops: 2,
    };
    let input = |author: &str, target: MentionTarget, quiet: bool, hop: u32| MentionDispatchInput {
        key: DispatchKey {
            trigger_sequence: 9,
        },
        conversation: DispatchConversation {
            desk_id: "eng".into(),
            thread_root: None,
        },
        author_id: author.into(),
        content: "@x take this".into(),
        mentions: vec![Mention {
            target,
            text: "@x".into(),
            offset: 0,
            quiet,
        }],
        hop,
    };
    let agent = |id: &str| MentionTarget::Agent { id: id.into() };
    let cases = [
        (
            "runs the target",
            on,
            input("alice", agent("bob"), false, 0),
        ),
        (
            "Disabled",
            MentionDispatchPolicy {
                enabled: false,
                ..on
            },
            input("alice", agent("bob"), false, 0),
        ),
        (
            "HopLimitReached",
            on,
            input("alice", agent("bob"), false, 2),
        ),
        ("SourceInactive", on, input("gus", agent("bob"), false, 0)),
        (
            "NoDirectAgentMention (a person)",
            on,
            input(
                "alice",
                MentionTarget::Person { id: "pat".into() },
                false,
                0,
            ),
        ),
        (
            "NoDirectAgentMention (everyone)",
            on,
            input("alice", MentionTarget::Everyone, false, 0),
        ),
        (
            "NoDirectAgentMention (quiet)",
            on,
            input("alice", agent("bob"), true, 0),
        ),
        ("SelfMention", on, input("alice", agent("alice"), false, 0)),
        ("TargetInactive", on, input("alice", agent("gus"), false, 0)),
        (
            "HopOverflow (unreachable)",
            MentionDispatchPolicy {
                enabled: true,
                max_hops: u32::MAX,
            },
            input("alice", agent("bob"), false, u32::MAX),
        ),
    ];
    for (label, policy, input) in cases {
        let verdict = match mention_dispatch(policy, &input, &roster)? {
            MentionDispatchDecision::One { request } => {
                format!("runs @{} at hop {}", request.target_id, request.child_hop)
            }
            MentionDispatchDecision::None { reason } => format!("none({reason:?}): {reason}"),
        };
        println!("  {label:<34} {verdict}");
    }
    println!("  NO_AVAILABLE_TARGET = {NO_AVAILABLE_TARGET:?}");
    Ok(())
}
