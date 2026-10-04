//! The team briefing and session initialization.

use tinyhivemind_core::aside::{AsidePolicy, Audience, Viewer};
use tinyhivemind_core::dispatch::MentionDispatchPolicy;
use tinyhivemind_core::runtime::{
    BriefingNote, BrevityPolicy, MentionDispatchContext, SessionQuery, TeamBriefing,
    THREAD_INDEX_LIMIT, THREAD_INDEX_SCAN, THREAD_OPENING_CHARS, initialize_session,
    initialize_session_with_context,
};
use tinyhivemind_lab::{MemoryLog, Res, agent, block_on, person, section};

use crate::{eng, world};

fn added(base: &str, other: &str) -> Vec<String> {
    other
        .lines()
        .filter(|line| !base.lines().any(|b| b == *line))
        .map(str::to_owned)
        .collect()
}

pub fn briefing() -> Res {
    section("briefing: TeamBriefing, BrevityPolicy, dispatch and aside rules");
    let world = world();
    let conv = eng();
    let roster = world.roster();
    let desks = world.desks();
    let base = TeamBriefing::from_snapshots("alice", &conv, &desks, &roster)?;
    let text = base.system_text();
    println!("{text}");

    let tuned = TeamBriefing {
        brevity: BrevityPolicy {
            message_chars: 80,
            window: 12,
        },
        asides: AsidePolicy {
            enabled: true,
            max_members: 2,
            max_messages: 3,
            must_surface: true,
            require_thread: false,
        },
        ..base.clone()
    };
    println!("\n  tuned brevity + asides add:");
    for line in added(&text, &tuned.system_text()) {
        println!("    + {line}");
    }
    let policy = MentionDispatchPolicy {
        enabled: true,
        max_hops: 2,
    };
    for hop in [0, 2] {
        let ctx = MentionDispatchContext { policy, hop };
        println!(
            "  dispatch hop {hop}/{}: may_dispatch={} adds {} line(s)",
            policy.max_hops,
            ctx.may_dispatch(),
            added(&text, &base.system_text_with_dispatch(ctx)).len()
        );
    }
    let off = MentionDispatchContext {
        policy: MentionDispatchPolicy {
            enabled: false,
            max_hops: 2,
        },
        hop: 0,
    };
    println!("  dispatch disabled: may_dispatch={}", off.may_dispatch());
    let brevity = BrevityPolicy::default();
    println!(
        "  BrevityPolicy::default overrun(700 chars) = {:?}, overrun(10 chars) = {:?}",
        brevity.overrun(&"x".repeat(700)),
        brevity.overrun("short")
    );

    let mut log = MemoryLog::default();
    let root = log.say("eng", person("sam"), "Why does checkout time out?");
    log.reply("eng", root, agent("alice"), "Looks like the gateway retry.");
    log.say("eng", agent("bob"), "!pin #retry gateway retries on 503");
    for n in 0..4 {
        log.append(
            "eng",
            None,
            agent("alice"),
            &format!("aside {n}"),
            Audience::Aside {
                members: vec!["bob".into()],
            },
        );
    }
    log.say("eng", agent("carol"), "Noted.");
    let query = SessionQuery {
        conversation: conv.clone(),
        viewer: Viewer::Agent { id: "carol".into() },
        before: None,
        window: 10,
    };
    let plain = block_on(initialize_session(&log, &query, base.clone()))?;
    println!(
        "  initialize_session: {} history rows, stated window {} (asked 10; elided asides shrink it)",
        plain.history.len(),
        plain.briefing.brevity.window
    );
    let note = BriefingNote {
        heading: "Earlier in this channel".into(),
        lines: vec!["[digest] checkout timeout under investigation".into()],
    };
    let rich = block_on(initialize_session_with_context(
        &log,
        &query,
        base,
        vec![note],
    ))?;
    println!("  context.system_text():");
    for line in rich.context.system_text().unwrap_or_default().lines() {
        println!("    {line}");
    }
    println!(
        "  THREAD_INDEX_LIMIT={THREAD_INDEX_LIMIT} THREAD_INDEX_SCAN={THREAD_INDEX_SCAN} THREAD_OPENING_CHARS={THREAD_OPENING_CHARS}"
    );
    Ok(())
}

