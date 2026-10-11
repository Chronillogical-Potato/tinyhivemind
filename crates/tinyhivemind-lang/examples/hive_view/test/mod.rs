//! Offline tests for `hive_view`: loading, rendering, runs, desk routing, the
//! frame wire form, the comparison, and the live host's request and reply
//! handling. No network and no clock; the symlink refusal alone writes a scratch
//! directory, removed afterwards.

mod args;
mod compare;
mod live;
mod packages;
mod record;
mod routing;
mod runs;
mod support;
mod wire;
