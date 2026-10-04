//! What a room remembers: digest, pins, sharing, briefing, elsewhere, threads.
//!
//! Everything is scripted and offline. A scripted `Digester` stands in for the
//! model, an in-memory log stands in for the host's journal, and every number
//! printed is a fold over those two.
//!
//! Run with `cargo run --bin memory_hive [-- --trace out.jsonl]`.

mod briefing;
mod digest;
mod elsewhere;
mod pins;
mod sharing;

use tinyhivemind_core::runtime::Conversation;
use tinyhivemind_lab::{Res, TraceRig, World};

pub(crate) fn eng() -> Conversation {
    Conversation {
        desk_id: "eng".into(),
        desk_name: "Engineering".into(),
        thread_root: None,
    }
}

pub(crate) fn world() -> World {
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


fn main() -> Res {
    let rig = TraceRig::from_args();
    digest::digest(&rig)?;
    pins::pins()?;
    sharing::sharing()?;
    briefing::briefing()?;
    elsewhere::elsewhere_and_threads()?;
    if let Some(path) = rig.path() {
        println!("\ntrace written to {path}");
    }
    Ok(())
}
