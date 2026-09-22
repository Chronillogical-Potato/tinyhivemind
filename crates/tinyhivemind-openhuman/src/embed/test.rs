//! What the embed runner asks of a runtime. Seating and running are proven
//! with the raw runner in `runner/test.rs`, on one process-wide runtime.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::{EmbedRunner, dispatchers};

#[test]
fn an_embed_seat_needs_mcp_boot_and_nothing_else() {
    let services = EmbedRunner::services();
    assert!(services.mcp_boot, "without it no seat is offered a tool");
    let none = openhuman_embed::ServiceSet::none();
    assert_eq!(
        format!("{services:?}").replace("mcp_boot: true", "mcp_boot: false"),
        format!("{none:?}"),
        "everything else is off"
    );
}

#[test]
fn the_road_is_the_three_dispatchers() {
    assert_eq!(
        dispatchers(),
        ["mcp_list_servers", "mcp_list_tools", "mcp_call_tool"]
    );
}
