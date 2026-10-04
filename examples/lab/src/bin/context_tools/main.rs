//! What a seat can say and what it can see: the room's tools and the projection.
//!
//! Core is not given `tinyhivemind-tools`, so the tool surface here is core's own
//! `runtime::speech`: the specs a host would render, `interpret` for a seat's
//! call, `commit_utterance` for the row that results, and the rejections a seat
//! reads inside its own turn. The second half shows what one participant sees of
//! the shared transcript: audience redaction, aside stubs, thread scope, code
//! masking, and mentions.
//!
//! Run with `cargo run --bin context_tools [-- --trace out.jsonl]`.

mod projection;
mod tools;

use tinyhivemind_lab::{Res, TraceRig};

fn main() -> Res {
    let rig = TraceRig::from_args();
    let tracer = rig.tracer("context-tools");
    tools::run(&tracer)?;
    projection::run()?;
    if let Some(path) = rig.path() {
        println!("\ntrace written to {path}");
    }
    Ok(())
}
