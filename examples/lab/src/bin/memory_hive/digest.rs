//! Channel compaction: planning, a scripted `Digester`, and every refusal.

use std::sync::Mutex;

use tinyhivemind_core::aside::{Audience, Viewer};
use tinyhivemind_core::runtime::digest::{
    ChannelDigest, ChannelHead, DigestFuture, DigestOutcome, DigestPlan, DigestPolicy,
    DigestRejection, DigestRequest, Digester, accept_digest, apply_digest, plan_digest, refold,
};
use tinyhivemind_core::runtime::pins::read_pinboard;
use tinyhivemind_core::runtime::{
    Conversation, PIN_LIMIT, SESSION_WINDOW, Sequence, SessionQuery, project_session,
};
use tinyhivemind_core::telemetry::TraceEvent;
use tinyhivemind_lab::{MemoryLog, Res, TraceRig, agent, block_on, section};

use crate::eng;

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

pub fn digest(rig: &TraceRig) -> Res {
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
