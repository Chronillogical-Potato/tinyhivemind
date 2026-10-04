//! The completion driver and the conductor, driven by scripted seats.
//!
//! `raw` pokes `CompletionDriver` one committed utterance at a time. `routing`
//! sweeps the router acceptance policy. `conduct` runs whole episodes through
//! `Conductor`, varies its walls and the driver's knobs, and replays each run
//! from every snapshot to prove resume is exact.
//!
//! Run with `cargo run --bin driver_knobs [-- --trace out.jsonl]`.

mod fixture;
mod raw;

use tinyhivemind_lab::{Res, TraceRig};

fn main() -> Res {
    let rig = TraceRig::from_args();
    raw::run()?;
    if let Some(path) = rig.path() {
        println!("\ntrace written to {path}");
    }
    Ok(())
}
