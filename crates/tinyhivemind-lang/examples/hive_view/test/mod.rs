//! Offline tests for `hive_view`: loading, rendering, runs, desk routing, the
//! frame wire form, the comparison, and the live host's request and reply
//! handling. No network, no clock, no temporary files.

mod args;
mod compare;
mod live;
mod packages;
mod routing;
mod runs;
mod support;
mod wire;
