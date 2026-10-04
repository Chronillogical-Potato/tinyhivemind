//! Who is here, who answers, and who may act: identity, responder, approval.
//!
//! The pure gates at the bottom of the stack, each driven through every
//! branch with hand-built snapshots. No transcript and no model.
//!
//! Run with `cargo run --bin gate_knobs`.

mod approval;
mod dispatch;
mod identity;
mod responder;

use tinyhivemind_lab::Res;

fn main() -> Res {
    identity::run()?;
    dispatch::run()?;
    responder::run()?;
    approval::run()?;
    Ok(())
}
