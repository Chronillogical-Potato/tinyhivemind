//! A bug handed across desks by referral, and what each policy knob does to it.
//!
//! Three desks can reach each other only through `referral`. A support seat
//! triages a bug, a semantic router picks the desk, the referral carries it
//! over, and the answer has to find its way back. Everything is scripted; the
//! "router" is a keyword counter and the "seats" are rules.
//!
//! Run with `cargo run --bin relay_hive [-- --trace out.jsonl]`.

mod gallery;
mod relay;

use tinyhivemind_lab::{Res, TraceRig, World};

pub(crate) fn world() -> World {
    World::new()
        .agent("alice")
        .agent("bob")
        .agent("carol")
        .agent("dan")
        .agent("erin")
        .agent("frank")
        .agent("gus")
        .agent("hal")
        .desk(
            "support",
            "Support",
            "Customers and triage",
            &["alice", "bob"],
        )
        .desk("backend", "Backend", "APIs and checkout", &["carol", "dan"])
        .desk(
            "infra",
            "Infrastructure",
            "Network, TLS and clusters",
            &["erin", "frank"],
        )
        .desk("ghosts", "Ghosts", "Nobody home", &["gus"])
        .retire("gus")
}

fn main() -> Res {
    let rig = TraceRig::from_args();
    relay::run(&rig)?;
    gallery::run()?;
    if let Some(path) = rig.path() {
        println!("\ntrace written to {path}");
    }
    Ok(())
}
