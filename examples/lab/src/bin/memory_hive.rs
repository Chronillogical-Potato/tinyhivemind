//! What a room remembers: digest, pins, sharing, briefing, elsewhere, threads.
//!
//! Everything is scripted and offline. A scripted `Digester` stands in for the
//! model, an in-memory log stands in for the host's journal, and every number
//! printed is a fold over those two.
//!
//! Run with `cargo run --bin memory_hive [-- --trace out.jsonl]`.

use std::sync::Mutex;

use tinyhivemind_core::aside::{AsidePolicy, Audience, Viewer};
use tinyhivemind_core::dispatch::MentionDispatchPolicy;
use tinyhivemind_core::runtime::digest::{
    ChannelDigest, ChannelHead, DigestFuture, DigestOutcome, DigestPlan, DigestPolicy,
    DigestRejection, DigestRequest, Digester, accept_digest, apply_digest, plan_digest, refold,
};
use tinyhivemind_core::runtime::pins::{
    PIN_EXCERPT_CHARS, PIN_LIMIT, PIN_MARKER_CAP, PIN_SCAN, fold_pins, pin_note, read_directives,
    read_pinboard,
};
use tinyhivemind_core::runtime::sharing::{
    PRESENT_SET_LIMIT, SharingPlan, SharingQuery, SharingState, initialized_state, note_present,
    prepare_delta,
};
use tinyhivemind_core::runtime::{
    BrevityPolicy, BriefingNote, Conversation, ElsewhereQuery, MentionDispatchContext, Pin,
    SESSION_WINDOW, Sequence, SessionQuery, THREAD_INDEX_LIMIT, THREAD_INDEX_SCAN,
    THREAD_OPENING_CHARS, TeamBriefing, gather_elsewhere, initialize_session,
    initialize_session_with_context, project_session, read_thread_index, render_row,
};
use tinyhivemind_core::telemetry::TraceEvent;
use tinyhivemind_lab::{MemoryLog, TraceRig, World, agent, block_on, person};

type Res = Result<(), Box<dyn std::error::Error>>;

/// How the scripted digester misbehaves.
#[derive(Clone, Copy)]
enum Mode {
    Honest,
    Blank,
    Verbose,
    Broken,
}

/// A digester with no model: it states what it was handed.
struct Scripted {
    mode: Mode,
    calls: Mutex<Vec<String>>,
}

impl Scripted {
    fn new(mode: Mode) -> Self {
        Self {
            mode,
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl Digester for Scripted {
    fn digest<'a>(&'a self, request: &'a DigestRequest) -> DigestFuture<'a> {
        let first = request.messages.first().map(|m| m.sequence.0).unwrap_or(0);
        let note = format!(
            "fold ^{first}..^{} rows={} prior={} pinned={:?}",
            request.through,
            request.messages.len(),
            request.prior.is_some(),
            request.pinned.iter().map(|s| s.0).collect::<Vec<_>>()
        );
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(note.clone());
        }
        let mode = self.mode;
        let budget = request.budget_chars;
        Box::pin(async move {
            match mode {
                Mode::Honest => Ok(format!("[digest] {note}")),
                Mode::Blank => Ok("   \n ".to_owned()),
                Mode::Verbose => Ok("x".repeat(budget + 1)),
                Mode::Broken => Err("the model is down".into()),
            }
        })
    }
}

fn section(title: &str) {
    println!("\n== {title} ==");
}

fn eng() -> Conversation {
    Conversation {
        desk_id: "eng".into(),
        desk_name: "Engineering".into(),
        thread_root: None,
    }
}

fn world() -> World {
    World::new()
        .agent("alice")
        .agent("bob")
        .agent("carol")
        .agent("dave")
        .agent("erin")
        .desk(
            "eng",
            "Engineering",
            "Build the product",
            &["alice", "bob", "carol"],
        )
        .desk("ops", "Operations", "Keep it running", &["dave", "erin"])
}

fn describe(outcome: &DigestOutcome) -> String {
    match outcome {
        DigestOutcome::Current => "Current".into(),
        DigestOutcome::Folded(d) => format!(
            "Folded(gen {}, through ^{}, covered {}): {}",
            d.generation, d.through, d.covered, d.text
        ),
        DigestOutcome::Unavailable => "Unavailable".into(),
        DigestOutcome::Rejected { reason } => format!("Rejected({reason:?})"),
    }
}

fn digest(rig: &TraceRig) -> Res {
    section("digest: plan_digest over DigestPolicy");
    let tracer = rig.tracer("memory-digest");
    let policy = DigestPolicy {
        keep_live: 10,
        fold_after: 8,
        fold_after_chars: 0,
        input_limit: 20,
        budget_chars: 300,
    };
    println!("policy {policy:?}");
    println!("default {:?}", DigestPolicy::default());
    println!(
        "from_token_budget(50).fold_after_chars = {} ({} chars per token)",
        DigestPolicy::from_token_budget(50).fold_after_chars,
        DigestPolicy::CHARS_PER_TOKEN
    );
    for head in [5_u64, 10, 18, 19, 40, 70] {
        let plan = plan_digest(None, ChannelHead::at(Sequence(head)), policy);
        println!("  head ^{head:<3} -> {}", plan_label(&plan));
    }
    let by_size = DigestPolicy {
        fold_after_chars: 400,
        ..policy
    };
    let small = ChannelHead {
        sequence: Sequence(16),
        unfolded_chars: 500,
    };
    println!(
        "  head ^16 with 500 unfolded chars, rows alone: {}; with fold_after_chars=400: {}",
        plan_label(&plan_digest(None, ChannelHead::at(Sequence(16)), policy)),
        plan_label(&plan_digest(None, small, by_size))
    );

    let mut log = MemoryLog::default();
    let authors = ["alice", "bob", "carol"];
    for n in 1..=70_u64 {
        let body = if n == 3 {
            "Decision: ship behind a flag.".to_owned()
        } else if n == 5 {
            "!pin ^3 #decision keep this".to_owned()
        } else if n == 7 {
            "private aside content".to_owned()
        } else {
            format!("status update number {n}")
        };
        let audience = if n == 7 {
            Audience::Aside {
                members: vec!["bob".into()],
            }
        } else {
            Audience::Desk
        };
        log.append(
            "eng",
            None,
            agent(authors[(n % 3) as usize]),
            &body,
            audience,
        );
    }
    let conv = eng();
    let pins = block_on(read_pinboard(
        &log,
        &conv,
        &Viewer::Operator,
        PIN_LIMIT,
        None,
    ))?;

    section("digest: refold with an honest scripted Digester, to a fixed point");
    let honest = Scripted::new(Mode::Honest);
    let mut account: Option<ChannelDigest> = None;
    let head = ChannelHead::at(log.head());
    for step in 1..=8 {
        let outcome = block_on(refold(
            &log,
            Some(&honest),
            &conv,
            account.as_ref(),
            head,
            &pins,
            policy,
        ))?;
        println!("  step {step}: {}", describe(&outcome));
        tracer.emit(TraceEvent::Mark {
            label: "digest.refold".into(),
            detail: describe(&outcome),
        });
        match outcome {
            DigestOutcome::Folded(next) => account = Some(next),
            _ => break,
        }
    }
    println!(
        "  digester was called {} times; pins handed to it ride in `pinned=`",
        honest.calls.lock().map(|c| c.len()).unwrap_or(0)
    );

    section("digest: apply_digest narrows a projected window");
    let window = block_on(project_session(
        &log,
        &SessionQuery {
            conversation: conv.clone(),
            viewer: Viewer::Agent { id: "carol".into() },
            before: None,
            window: SESSION_WINDOW,
        },
    ))?;
    let narrowed = apply_digest(account.as_ref(), &window);
    println!(
        "  window {} rows -> {} rows after the account; covered through {:?}",
        window.len(),
        narrowed.messages.len(),
        narrowed.covered_through
    );
    println!(
        "  without an account the window is untouched: {}",
        apply_digest(None, &window).messages.len()
    );

    section("digest: every way a fold fails to become an account");
    for (label, mode) in [
        ("blank", Mode::Blank),
        ("verbose", Mode::Verbose),
        ("broken", Mode::Broken),
    ] {
        let scripted = Scripted::new(mode);
        let out = block_on(refold(
            &log,
            Some(&scripted),
            &conv,
            None,
            head,
            &pins,
            policy,
        ))?;
        println!("  {label:<8}-> {}", describe(&out));
    }
    let none = block_on(refold(&log, None, &conv, None, head, &pins, policy))?;
    println!("  no digester -> {}", describe(&none));
    let held = account.clone();
    let request = DigestRequest {
        conversation: conv.clone(),
        prior: None,
        messages: Vec::new(),
        through: Sequence(1),
        budget_chars: 10,
        pinned: Vec::new(),
    };
    println!(
        "  regressed   -> {:?}",
        accept_digest(held.as_ref(), &request, "older")
    );
    println!(
        "  too large   -> {:?}",
        accept_digest(None, &request, "this text is longer than ten")
    );
    println!("  empty       -> {:?}", accept_digest(None, &request, "  "));
    assert_eq!(
        accept_digest(None, &request, " "),
        Err(DigestRejection::Empty)
    );

    section("digest: typed errors");
    let other = Conversation {
        desk_id: "ops".into(),
        desk_name: "Operations".into(),
        thread_root: None,
    };
    let wrong = block_on(refold(
        &log,
        Some(&honest),
        &other,
        account.as_ref(),
        head,
        &pins,
        policy,
    ));
    println!(
        "  account of another channel: {}",
        wrong.err().map_or("ok".into(), |e| e.to_string())
    );
    let mut deep = MemoryLog::default();
    for n in 1..=2100 {
        deep.say("eng", agent("alice"), &format!("row {n}"));
    }
    let gap = tinyhivemind_core::runtime::digest::collect_digest_input(
        &deep,
        &conv,
        Some(Sequence(1)),
        deep.head(),
    );
    println!(
        "  scan past SCAN_LIMIT: {}",
        block_on(gap).err().map_or("ok".into(), |e| e.to_string())
    );
    Ok(())
}

fn plan_label(plan: &DigestPlan) -> String {
    match plan {
        DigestPlan::Current => "Current".into(),
        DigestPlan::Fold { after, through } => {
            format!("Fold after {:?} through ^{through}", after.map(|s| s.0))
        }
    }
}

fn pins() -> Res {
    section("pins: !pin / !unpin, fences, asides, limits");
    println!(
        "constants: PIN_LIMIT={PIN_LIMIT} PIN_SCAN={PIN_SCAN} PIN_EXCERPT_CHARS={PIN_EXCERPT_CHARS} PIN_MARKER_CAP={PIN_MARKER_CAP}"
    );
    let mut log = MemoryLog::default();
    log.say(
        "eng",
        agent("alice"),
        "The rate limiter resets at midnight UTC.",
    );
    log.say("eng", agent("bob"), "Use sqlx for the new service.");
    log.say("eng", agent("carol"), "!pin ^1 #limits resets at midnight");
    log.say("eng", agent("bob"), "!pin ^2 #stack");
    log.say("eng", agent("alice"), "!unpin ^2");
    log.say("eng", agent("carol"), "```\n!pin ^1 #fenced\n```");
    let secret = log.append(
        "eng",
        None,
        agent("alice"),
        "ship friday, tell nobody",
        Audience::Aside {
            members: vec!["bob".into()],
        },
    );
    log.append(
        "eng",
        None,
        agent("alice"),
        &format!("!pin ^{secret} #secret"),
        Audience::Aside {
            members: vec!["bob".into()],
        },
    );
    log.say("eng", agent("bob"), "!pin #self a long opening that goes on and on and on and on and on and on and on and on and on and on and on and on and on and on");
    log.say("eng", agent("bob"), "!unpin");
    let viewers = [
        ("operator", Viewer::Operator),
        ("bob (in the aside)", Viewer::Agent { id: "bob".into() }),
        ("carol (outside it)", Viewer::Agent { id: "carol".into() }),
    ];
    for (name, viewer) in &viewers {
        let board = fold_pins(log.rows(), viewer, PIN_LIMIT);
        println!("  {name}: {}", board_line(&board));
    }
    println!(
        "  limit 1 (newest marker wins): {}",
        board_line(&fold_pins(log.rows(), &Viewer::Operator, 1))
    );
    println!(
        "  limit 0: {}",
        board_line(&fold_pins(log.rows(), &Viewer::Operator, 0))
    );
    let tail: Vec<_> = log.rows().iter().skip(2).cloned().collect();
    println!(
        "  rows scanned from ^3 only, so ^1 is outside the scan: {}",
        board_line(&fold_pins(&tail, &Viewer::Operator, PIN_LIMIT))
    );
    let conv = eng();
    let early = block_on(read_pinboard(
        &log,
        &conv,
        &Viewer::Operator,
        PIN_LIMIT,
        Some(Sequence(5)),
    ))?;
    println!("  read_pinboard before ^5: {}", board_line(&early));
    let board = block_on(read_pinboard(
        &log,
        &conv,
        &Viewer::Operator,
        PIN_LIMIT,
        None,
    ))?;
    if let Some(note) = pin_note(&board) {
        println!("  pin_note -> {}:", note.heading);
        for line in note.lines {
            println!("    {line}");
        }
    }
    println!("  pin_note of an empty board: {:?}", pin_note(&[]));

    let storm: String = (1..=12).map(|n| format!("!pin ^{n} #m{n}\n")).collect();
    let found = read_directives(&storm, &agent("alice"), Sequence(99));
    println!(
        "  12 markers in one message yield {} (cap {PIN_MARKER_CAP})",
        found.len()
    );
    println!(
        "  `!unpin` with no target yields {} directives",
        read_directives("!unpin", &agent("alice"), Sequence(1)).len()
    );
    Ok(())
}

fn board_line(board: &[Pin]) -> String {
    let parts: Vec<String> = board
        .iter()
        .map(|p| {
            format!(
                "^{}{}{}",
                p.sequence,
                p.label
                    .as_deref()
                    .map(|l| format!("#{l}"))
                    .unwrap_or_default(),
                if p.excerpt.is_none() {
                    "(no excerpt)"
                } else {
                    ""
                }
            )
        })
        .collect();
    if parts.is_empty() {
        "(empty)".into()
    } else {
        parts.join(" ")
    }
}

fn sharing() -> Res {
    section("sharing: prepare_delta, watermark, ReinitializeReason");
    let mut log = MemoryLog::default();
    for n in 1..=5 {
        log.say("eng", agent("alice"), &format!("history {n}"));
    }
    log.say("eng", agent("alice"), "visible new row");
    log.append(
        "eng",
        None,
        agent("alice"),
        "whisper to bob",
        Audience::Aside {
            members: vec!["bob".into()],
        },
    );
    log.say("eng", agent("bob"), "another new row");
    let conv = eng();
    let state = initialized_state(conv.clone(), Sequence(5));
    for (who, id) in [("bob", "bob"), ("carol", "carol")] {
        let viewer = Viewer::Agent { id: id.into() };
        let plan = block_on(prepare_delta(
            &log,
            &SharingQuery {
                desired_conversation: &conv,
                current_conversation: &conv,
                state: &state,
                viewer: &viewer,
                before: Sequence(9),
            },
        ))?;
        if let SharingPlan::Delta(delta) = plan {
            let rows: Vec<String> = delta
                .messages
                .iter()
                .map(|m| {
                    format!(
                        "^{}:{}",
                        m.sequence,
                        if m.elided.is_some() {
                            "<elided>"
                        } else {
                            "text"
                        }
                    )
                })
                .collect();
            println!(
                "  {who}: delta {rows:?} next watermark ^{}",
                delta.next_state.watermark
            );
        }
    }
    let mut seeded = state.clone();
    note_present(&mut seeded, Sequence(6))?;
    let viewer = Viewer::Agent { id: "bob".into() };
    let plan = block_on(prepare_delta(
        &log,
        &SharingQuery {
            desired_conversation: &conv,
            current_conversation: &conv,
            state: &seeded,
            viewer: &viewer,
            before: Sequence(9),
        },
    ))?;
    if let SharingPlan::Delta(delta) = plan {
        println!(
            "  after note_present(^6): delta carries {:?}",
            delta
                .messages
                .iter()
                .map(|m| m.sequence.0)
                .collect::<Vec<_>>()
        );
    }
    let other = Conversation {
        thread_root: Some(Sequence(2)),
        ..conv.clone()
    };
    let changed = block_on(prepare_delta(
        &log,
        &SharingQuery {
            desired_conversation: &other,
            current_conversation: &conv,
            state: &state,
            viewer: &viewer,
            before: Sequence(9),
        },
    ))?;
    println!("  conversation changed -> {changed:?}");

    let mut deep = MemoryLog::default();
    for n in 1..=2100 {
        deep.say("eng", agent("alice"), &format!("row {n}"));
    }
    let far = initialized_state(conv.clone(), Sequence(1));
    let gap = block_on(prepare_delta(
        &deep,
        &SharingQuery {
            desired_conversation: &conv,
            current_conversation: &conv,
            state: &far,
            viewer: &viewer,
            before: Sequence(2101),
        },
    ))?;
    println!("  watermark beyond the scan -> {gap:?}");

    let mut compacted = MemoryLog::starting_after(49);
    compacted.say("eng", agent("alice"), "oldest surviving row");
    compacted.say("eng", agent("alice"), "newer");
    let behind = initialized_state(conv.clone(), Sequence(20));
    let missing = block_on(prepare_delta(
        &compacted,
        &SharingQuery {
            desired_conversation: &conv,
            current_conversation: &conv,
            state: &behind,
            viewer: &viewer,
            before: Sequence(52),
        },
    ))?;
    println!("  log compacted past the watermark -> {missing:?}");

    let regress = block_on(prepare_delta(
        &log,
        &SharingQuery {
            desired_conversation: &conv,
            current_conversation: &conv,
            state: &state,
            viewer: &viewer,
            before: Sequence(3),
        },
    ));
    println!(
        "  before below watermark -> error: {}",
        regress.err().map_or("none".into(), |e| e.to_string())
    );
    let mut full = initialized_state(conv.clone(), Sequence(0));
    let mut overflow = None;
    for n in 1..=(PRESENT_SET_LIMIT as u64 + 1) {
        if let Err(error) = note_present(&mut full, Sequence(n)) {
            overflow = Some(error.to_string());
        }
    }
    println!(
        "  note_present past PRESENT_SET_LIMIT={PRESENT_SET_LIMIT} -> {}",
        overflow.unwrap_or_default()
    );
    let wire = format!(
        "{{\"conversation\":{},\"watermark\":0,\"present_above_watermark\":[{}]}}",
        serde_json::to_string(&conv)?,
        (1..=PRESENT_SET_LIMIT + 1)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    println!(
        "  decoding an oversized SharingState: {}",
        serde_json::from_str::<SharingState>(&wire)
            .err()
            .map_or("accepted".into(), |e| e.to_string())
    );
    Ok(())
}

fn added(base: &str, other: &str) -> Vec<String> {
    other
        .lines()
        .filter(|line| !base.lines().any(|b| b == *line))
        .map(str::to_owned)
        .collect()
}

fn briefing() -> Res {
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

fn elsewhere_and_threads() -> Res {
    section("elsewhere: what a seat sees of the desks it is not on");
    let world = world();
    let mut log = MemoryLog::default();
    log.say("eng", agent("alice"), "Deploying the gateway at noon.");
    log.say("ops", agent("dave"), "Pager is quiet.");
    log.say("ops", agent("erin"), "Disk on db-2 at 91%.");
    log.append(
        "ops",
        None,
        agent("dave"),
        "private to erin",
        Audience::Aside {
            members: vec!["erin".into()],
        },
    );
    let conversations = [
        eng(),
        Conversation {
            desk_id: "ops".into(),
            desk_name: "Operations".into(),
            thread_root: None,
        },
    ];
    let current = eng();
    let seen = block_on(gather_elsewhere(
        &log,
        &ElsewhereQuery {
            seat: "alice",
            conversations: &conversations,
            current: Some(&current),
            before: None,
            window: 5,
        },
    ))?;
    for e in &seen {
        println!(
            "  alice sees {} (desk {}):",
            e.conversation.desk_name, e.conversation.desk_id
        );
        for row in &e.rows {
            println!(
                "    {}",
                render_row(row).unwrap_or_else(|| "<elided aside stub>".into())
            );
        }
    }
    let unrestricted = block_on(gather_elsewhere(
        &log,
        &ElsewhereQuery {
            seat: "alice",
            conversations: &conversations[..1],
            current: None,
            before: None,
            window: 5,
        },
    ))?;
    println!(
        "  alice may read eng from outside it; nothing checks desk membership: {} row(s) (members: {:?})",
        unrestricted[0].rows.len(),
        world.desk_records()[1].members
    );

    section("threads: the index a viewer gets of a desk");
    let mut log = MemoryLog::default();
    let a = log.say(
        "eng",
        person("sam"),
        "Why does checkout time out under load?",
    );
    let b = log.say(
        "eng",
        person("sam"),
        &format!("Second thread {}", "with a rather long opening ".repeat(4)),
    );
    log.reply("eng", a, agent("alice"), "Gateway retry.");
    log.reply("eng", a, agent("bob"), "Confirmed.");
    log.reply("eng", b, agent("carol"), "Looking.");
    log.append(
        "eng",
        None,
        agent("alice"),
        "private root",
        Audience::Aside {
            members: vec!["bob".into()],
        },
    );
    let conv = eng();
    for (name, viewer) in [
        ("operator", Viewer::Operator),
        ("carol", Viewer::Agent { id: "carol".into() }),
    ] {
        let mut index = block_on(read_thread_index(&log, &conv, &viewer, THREAD_INDEX_LIMIT))?;
        // `landed` is board state core does not hold: the host fills it in.
        for line in &mut index {
            if line.root == a {
                line.landed = Some("PR #41".into());
            }
        }
        println!("  {name}:");
        for line in &index {
            println!(
                "    [{}] {:?} replies={} latest=^{} landed={:?}",
                line.root, line.opening, line.replies, line.latest, line.landed
            );
        }
    }
    println!(
        "  limit 1: {} line(s); inside a thread: {} line(s); limit 0: {} line(s)",
        block_on(read_thread_index(&log, &conv, &Viewer::Operator, 1))?.len(),
        block_on(read_thread_index(
            &log,
            &Conversation {
                thread_root: Some(a),
                ..conv.clone()
            },
            &Viewer::Operator,
            5
        ))?
        .len(),
        block_on(read_thread_index(&log, &conv, &Viewer::Operator, 0))?.len()
    );
    Ok(())
}

fn main() -> Res {
    let rig = TraceRig::from_args();
    digest(&rig)?;
    pins()?;
    sharing()?;
    briefing()?;
    elsewhere_and_threads()?;
    if let Some(path) = rig.path() {
        println!("\ntrace written to {path}");
    }
    Ok(())
}
