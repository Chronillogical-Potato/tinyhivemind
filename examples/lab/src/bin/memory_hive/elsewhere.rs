//! Reading other desks, and the thread index of one.

use tinyhivemind_core::aside::{Audience, Viewer};
use tinyhivemind_core::runtime::{
    Conversation, ElsewhereQuery, THREAD_INDEX_LIMIT, gather_elsewhere, read_thread_index,
    render_row,
};
use tinyhivemind_lab::{MemoryLog, Res, agent, block_on, person, section};

use crate::{eng, world};

pub fn elsewhere_and_threads() -> Res {
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

