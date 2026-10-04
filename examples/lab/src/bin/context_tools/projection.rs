//! What one participant sees of the transcript: redaction, scope, masking.

use tinyhivemind_core::aside::{Audience, Viewer};
use tinyhivemind_core::dispatch::{
    DispatchConversation, DispatchKey, MentionDispatchDecision, MentionDispatchInput,
    MentionDispatchPolicy, mention_dispatch,
};
use tinyhivemind_core::masking::{code_ranges, fenced_ranges, is_masked};
use tinyhivemind_core::mention::{
    MENTION_CAP, MentionAuthor, direct_responder, mentioned_members, resolve,
};
use tinyhivemind_core::runtime::{
    Conversation, Sequence, SessionMessage, SessionQuery, project_as, project_session,
};
use tinyhivemind_lab::{MemoryLog, Res, World, agent, block_on, person, section};

fn render(rows: &[SessionMessage]) -> String {
    rows.iter()
        .map(|m| match &m.elided {
            Some(e) => format!(
                "^{}..^{} <aside x{} settled_at={:?}>",
                m.sequence,
                e.through,
                e.messages,
                e.settled_at.map(|s| s.0)
            ),
            None => format!("^{}", m.sequence),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn run() -> Res {
    section("projection: one transcript, five viewers");
    let mut log = MemoryLog::default();
    let root = log.say("eng", person("sam"), "Kickoff: why is checkout slow?");
    let to = |id: &str| Audience::Aside {
        members: vec![id.into()],
    };
    log.append(
        "eng",
        None,
        agent("alice"),
        "I think it is the retries.",
        to("bob"),
    );
    log.append(
        "eng",
        None,
        agent("bob"),
        "Agree, check the 503 path.",
        to("alice"),
    );
    log.say(
        "eng",
        agent("alice"),
        "Surfacing: retries on 503 are the cause.",
    );
    log.reply("eng", root, agent("carol"), "Reading the gateway logs now.");
    log.say(
        "eng",
        person("sam"),
        "Second question: who owns the gateway?",
    );
    let conv = Conversation {
        desk_id: "eng".into(),
        desk_name: "Engineering".into(),
        thread_root: None,
    };
    let query = |viewer: Viewer, window: usize| SessionQuery {
        conversation: conv.clone(),
        viewer,
        before: None,
        window,
    };
    for (label, viewer) in [
        ("operator", Viewer::Operator),
        ("person sam", Viewer::Person { id: "sam".into() }),
        ("agent alice (author)", Viewer::Agent { id: "alice".into() }),
        ("agent bob (member)", Viewer::Agent { id: "bob".into() }),
        (
            "agent carol (outsider)",
            Viewer::Agent { id: "carol".into() },
        ),
    ] {
        let rows = block_on(project_session(&log, &query(viewer.clone(), 30)))?;
        println!("  {label:<24} {}", render(&rows));
    }
    let carol = Viewer::Agent { id: "carol".into() };
    println!(
        "  window 2 for carol: {}",
        render(&block_on(project_session(&log, &query(carol.clone(), 2)))?)
    );
    println!(
        "  window 0 for carol: {} rows",
        block_on(project_session(&log, &query(carol.clone(), 0)))?.len()
    );
    let thread = Conversation {
        thread_root: Some(root),
        ..conv.clone()
    };
    let in_thread = block_on(project_session(
        &log,
        &SessionQuery {
            conversation: thread,
            viewer: carol.clone(),
            before: None,
            window: 30,
        },
    ))?;
    println!("  thread-scoped on ^{root}: {}", render(&in_thread));
    let before = block_on(project_session(
        &log,
        &SessionQuery {
            before: Some(Sequence(4)),
            ..query(carol.clone(), 30)
        },
    ))?;
    println!("  carol before ^4: {}", render(&before));
    let all = block_on(project_session(&log, &query(Viewer::Operator, 30)))?;
    println!(
        "  project_as(operator rows, carol): {}",
        render(&project_as(&all, &carol))
    );
    println!(
        "  readable() on a stub is None: {}",
        all.iter().any(|m| m.readable().is_none())
            || project_as(&all, &carol)
                .iter()
                .any(|m| m.readable().is_none())
    );
    println!(
        "  Viewer helpers: operator.reads_everything={} agent_id={:?}; Audience::Aside members={:?}",
        Viewer::Operator.reads_everything(),
        carol.agent_id(),
        to("bob").members()
    );

    section("masking and mentions");
    let world = World::new()
        .agent("alice")
        .agent("bob")
        .agent("carol")
        .desk("eng", "Engineering", "Build it", &["alice", "bob", "carol"]);
    let roster = world.roster();
    let desks = world.desks();
    let body = "Ask @bob, not `@carol`, and\n```\n@alice stays quiet\n```\nthen @Engineering and @everyone";
    let masked = code_ranges(body);
    println!(
        "  fenced ranges {:?}; all code ranges {:?}",
        fenced_ranges(body),
        masked
    );
    println!(
        "  is_masked(at `@carol`)={} is_masked(at @bob)={}",
        is_masked(body.find("@carol").unwrap_or(0), &masked),
        is_masked(body.find("@bob").unwrap_or(0), &masked)
    );
    let found = resolve(
        body,
        None,
        &MentionAuthor::Agent { id: "alice".into() },
        &roster,
        &desks,
    );
    println!(
        "  resolve -> {:?}",
        found
            .iter()
            .map(|m| (m.text.as_str(), m.quiet))
            .collect::<Vec<_>>()
    );
    println!(
        "  direct_responder -> {:?}",
        direct_responder(&found, &roster)
    );
    println!(
        "  mentioned_members(addressed desk eng, responder bob) -> {:?}",
        mentioned_members(&found, Some("eng"), Some("bob"), &roster, &desks)
    );
    let stale = resolve(
        "hi @zed",
        Some(vec![tinyhivemind_core::mention::Mention {
            target: tinyhivemind_core::mention::MentionTarget::Agent { id: "zed".into() },
            text: "@zed".into(),
            offset: 3,
            quiet: false,
        }]),
        &MentionAuthor::Other,
        &roster,
        &desks,
    );
    println!(
        "  a supplied mention of an unknown agent: {:?}",
        stale
            .iter()
            .map(|m| (m.text.as_str(), m.quiet))
            .collect::<Vec<_>>()
    );
    let crowd = (1..=MENTION_CAP + 10).fold(World::new(), |w, n| w.agent(&format!("a{n}")));
    let crowd_roster = crowd.roster();
    let crowd_desks = crowd.desks();
    let flood: String = (1..=MENTION_CAP + 10).map(|n| format!("@a{n} ")).collect();
    let flooded = resolve(
        &flood,
        None,
        &MentionAuthor::Person { id: "sam".into() },
        &crowd_roster,
        &crowd_desks,
    );
    println!(
        "  {} distinct mentions: {} ping, {} made quiet (MENTION_CAP={MENTION_CAP})",
        flooded.len(),
        flooded.iter().filter(|m| !m.quiet).count(),
        flooded.iter().filter(|m| m.quiet).count()
    );
    let repeated = resolve(
        &"@bob ".repeat(5),
        None,
        &MentionAuthor::Other,
        &roster,
        &desks,
    );
    println!(
        "  @bob five times: {} ping (repeats are quiet)",
        repeated.iter().filter(|m| !m.quiet).count()
    );

    section("mention_dispatch: one bounded child turn");
    let input = |hop: u32, author: &str| MentionDispatchInput {
        key: DispatchKey {
            trigger_sequence: 11,
        },
        conversation: DispatchConversation {
            desk_id: "eng".into(),
            thread_root: None,
        },
        author_id: author.into(),
        content: "@bob take this".into(),
        mentions: resolve(
            "@bob take this",
            None,
            &MentionAuthor::Agent { id: author.into() },
            &roster,
            &desks,
        ),
        hop,
    };
    for (label, policy, input) in [
        (
            "enabled, hop 0",
            MentionDispatchPolicy {
                enabled: true,
                max_hops: 2,
            },
            input(0, "alice"),
        ),
        (
            "hop at the cap",
            MentionDispatchPolicy {
                enabled: true,
                max_hops: 2,
            },
            input(2, "alice"),
        ),
        (
            "disabled",
            MentionDispatchPolicy {
                enabled: false,
                max_hops: 2,
            },
            input(0, "alice"),
        ),
        (
            "self mention",
            MentionDispatchPolicy {
                enabled: true,
                max_hops: 2,
            },
            input(0, "bob"),
        ),
    ] {
        match mention_dispatch(policy, &input, &roster)? {
            MentionDispatchDecision::One { request } => println!(
                "  {label:<16} -> {} runs next (child hop {})",
                request.target_id, request.child_hop
            ),
            MentionDispatchDecision::None { reason } => println!("  {label:<16} -> none: {reason}"),
        }
    }
    Ok(())
}
