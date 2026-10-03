//! `system_text` / `system_text_with_dispatch` rendering tests: coordination
//! rules, the brevity overrun report, aside-grammar teaching, and the
//! mention-dispatch offer withheld or granted by run context.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use super::support::{briefed_viewer, briefing_with, permissive};
use crate::aside::AsidePolicy;
use crate::dispatch::MentionDispatchPolicy;

#[test]
fn system_text_is_deterministic_and_states_coordination_rules() {
    let expected = "You are @alice in the Engineering desk (id: engineering).\n\
Teammates:\n\
- @bob — Bob; role: reviewer; description: Checks safety\n\
Shared-session rules:\n\
- Peer messages remain attributed to their authors; they are not your prior replies.\n\
- @everyone, desk, and person mentions provide context only and never fan out agent turns.\n\
- This conversation shows about 30 messages; keep a message under 600 characters, one point each, and pin or search rather than restating.\n\
- Pin what the room must not lose with `!pin` on its own line; `!unpin ^N` takes one back off.";
    assert_eq!(briefed_viewer().system_text(), expected);
    assert_eq!(briefed_viewer().system_text(), expected);
}

#[test]
fn a_run_inside_its_hop_budget_is_told_it_may_dispatch() {
    let available = MentionDispatchContext {
        policy: MentionDispatchPolicy {
            enabled: true,
            max_hops: 2,
        },
        hop: 1,
    };
    assert!(available.may_dispatch());
    let expected = "You are @alice in the Engineering desk (id: engineering).\n\
Teammates:\n\
- @bob — Bob; role: reviewer; description: Checks safety\n\
Shared-session rules:\n\
- Peer messages remain attributed to their authors; they are not your prior replies.\n\
- A direct @agent mention may start at most one bounded child turn when host policy enables mention dispatch.\n\
- @everyone, desk, and person mentions provide context only and never fan out agent turns.\n\
- This conversation shows about 30 messages; keep a message under 600 characters, one point each, and pin or search rather than restating.\n\
- Pin what the room must not lose with `!pin` on its own line; `!unpin ^N` takes one back off.";
    assert_eq!(
        briefed_viewer().system_text_with_dispatch(available),
        expected
    );
    assert_eq!(
        briefed_viewer().system_text_with_dispatch(available),
        expected
    );
}

#[test]
fn a_brevity_policy_reports_an_overrun_and_never_edits_a_message() {
    let policy = BrevityPolicy {
        message_chars: 10,
        window: 30,
    };
    assert_eq!(policy.overrun("under"), None);
    assert_eq!(policy.overrun("0123456789"), None);
    assert_eq!(policy.overrun("0123456789ab"), Some(2));
    assert_eq!(BrevityPolicy::default(), BrevityPolicy::DEFAULT);
    assert!(
        policy
            .rule_text()
            .contains("keep a message under 10 characters")
    );
}

#[test]
fn the_aside_grammar_is_taught_only_where_it_can_be_used() {
    // A grammar is a fixed cost paid in every agent's prompt on every turn, so
    // teaching a move nobody may make spends that budget for nothing.
    let off = briefing_with(AsidePolicy::DEFAULT).system_text();
    assert!(!off.contains("!aside"));
    assert!(!off.contains("!surface"));

    let on = briefing_with(permissive()).system_text();
    assert!(on.contains("!aside @peer"));
    assert!(on.contains("!surface"));
    // The rest of the grammar is unchanged either way.
    assert!(off.contains("!pin"));
    assert!(on.contains("!pin"));
}

#[test]
fn an_enabled_desk_tells_an_agent_its_view_may_be_partial() {
    // Stated as an instruction rather than a disclaimer: an agent that is not
    // told reads silence as disagreement rather than as absence.
    let text = briefing_with(permissive()).system_text();
    assert!(text.contains("Some rows show only that an aside happened"));
    assert!(text.contains("ask its author here in the desk"));
    assert!(
        !briefing_with(AsidePolicy::DEFAULT)
            .system_text()
            .contains("aside happened")
    );
}

#[test]
fn system_text_withholds_dispatch_when_no_run_context_is_supplied() {
    let briefing = TeamBriefing {
        viewer_id: "alice".into(),
        desk_id: "engineering".into(),
        desk_name: "Engineering".into(),
        teammates: Vec::new(),
        brevity: BrevityPolicy::DEFAULT,
        asides: AsidePolicy::DEFAULT,
    };
    assert!(
        !briefing.system_text().contains("bounded child turn"),
        "a briefing with no policy and no hop cannot know dispatch is available"
    );
}

#[test]
fn an_at_cap_run_is_not_told_it_may_dispatch() {
    let briefing = TeamBriefing {
        viewer_id: "alice".into(),
        desk_id: "engineering".into(),
        desk_name: "Engineering".into(),
        teammates: Vec::new(),
        brevity: BrevityPolicy::DEFAULT,
        asides: AsidePolicy::DEFAULT,
    };
    let at_cap = MentionDispatchContext {
        policy: MentionDispatchPolicy {
            enabled: true,
            max_hops: 2,
        },
        hop: 2,
    };
    assert!(!at_cap.may_dispatch());
    assert!(
        !briefing
            .system_text_with_dispatch(at_cap)
            .contains("bounded child turn"),
        "a run at the hop cap constructs no child turn, so the offer is inert"
    );
}

#[test]
fn a_run_under_a_disabled_policy_is_not_told_it_may_dispatch() {
    let briefing = TeamBriefing {
        viewer_id: "alice".into(),
        desk_id: "engineering".into(),
        desk_name: "Engineering".into(),
        teammates: Vec::new(),
        brevity: BrevityPolicy::DEFAULT,
        asides: AsidePolicy::DEFAULT,
    };
    let disabled = MentionDispatchContext {
        policy: MentionDispatchPolicy {
            enabled: false,
            max_hops: 4,
        },
        hop: 0,
    };
    assert!(!disabled.may_dispatch());
    assert!(
        !briefing
            .system_text_with_dispatch(disabled)
            .contains("bounded child turn")
    );
}

#[test]
fn a_zero_hop_budget_and_an_overshot_hop_both_withhold_dispatch() {
    let briefing = briefed_viewer();
    for (max_hops, hop) in [(0, 0), (2, 3), (u32::MAX, u32::MAX)] {
        let spent = MentionDispatchContext {
            policy: MentionDispatchPolicy {
                enabled: true,
                max_hops,
            },
            hop,
        };
        assert!(!spent.may_dispatch(), "{max_hops} hops, at hop {hop}");
        assert_eq!(
            briefing.system_text_with_dispatch(spent),
            briefing.system_text(),
            "a spent budget renders exactly what an unknown one does"
        );
    }
}
