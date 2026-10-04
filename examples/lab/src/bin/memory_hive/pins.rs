//! Pinning: markers, fences, aside visibility, limits and the briefing note.

use tinyhivemind_core::aside::{Audience, Viewer};
use tinyhivemind_core::runtime::pins::{
    PIN_EXCERPT_CHARS, PIN_LIMIT, PIN_MARKER_CAP, PIN_SCAN, fold_pins, pin_note, read_directives,
    read_pinboard,
};
use tinyhivemind_core::runtime::{Pin, Sequence};
use tinyhivemind_lab::{MemoryLog, Res, agent, block_on, section};

use crate::eng;

pub fn pins() -> Res {
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
