//! Sweep the knobs of the deliberation algebra and print what each one buys.
//!
//! Five scripted seats argue two plans and a third. `hive::step` decides who
//! speaks; the seats' lines are a pure function of what their projection lets
//! them read. No model, no clock, no randomness: the same table every run.
//!
//! Run with `cargo run --bin swarm_knobs [-- --trace out.jsonl]`.

mod attention;
mod consensus;
mod division;
mod market;
mod room;
mod sweep;

use tinyhivemind_lab::{Res, TraceRig};

fn main() -> Res {
    let rig = TraceRig::from_args();
    sweep::run(&rig)?;
    market::run(&rig)?;
    if let Some(path) = rig.path() {
        println!("\ntrace written to {path}");
    }
    Ok(())
}
