//! Reading other desks, and the thread index of one.

use tinyhivemind_core::aside::{Audience, Viewer};
use tinyhivemind_core::runtime::{
    Sequence,
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
    let earlier = block_on(gather_elsewhere(
        &log,
        &ElsewhereQuery {
            seat: "alice",
            conversations: &conversations,
            current: Some(&current),
            before: Some(Sequence(3)),
            window: 1,
        },
    ))?;
    println!(
        "  before ^3 with window 1: {} row(s) of ops",
        earlier.iter().map(|e| e.rows.len()).sum::<usize>()
    );
    // Nothing in the query says which desks the seat sits on: dave is on ops,
    // and `gather_elsewhere` reads eng for him all the same.
    let unrestricted = block_on(gather_elsewhere(
        &log,
        &ElsewhereQuery {
            seat: "dave",
            conversations: &conversations[..1],
            current: None,
            before: None,
            window: 5,
        },
    ))?;
    println!(
        "  dave (desks: {:?}) is handed {} row(s) of eng; membership is the caller's check",
        world.desk_records()[1].members,
        unrestricted[0].rows.len()
    );
    // Conversation::equivalent_to treats `main` and `General` as one desk;
    // gather_elsewhere compares raw ids, so it does not skip the current one.
    let main_desk = Conversation {
        desk_id: "main".into(),
        desk_name: "General".into(),
        thread_root: None,
    };
    let alias = Conversation {
        desk_id: "general".into(),
        desk_name: "General".into(),
        thread_root: None,
    };
    println!(
        "  equivalent_to says main==general: {}",
        main_desk.equivalent_to(&alias)
    );
    let both = [alias];
    let leaked = block_on(gather_elsewhere(
        &log,
        &ElsewhereQuery {
            seat: "alice",
            conversations: &both,
            current: Some(&main_desk),
            before: None,
            window: 5,
        },
    ))?;
    println!(
        "  gather_elsewhere with current=main and listed=general returns {} conversation(s) (0 expected if aliases matched)",
        leaked.len()
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
