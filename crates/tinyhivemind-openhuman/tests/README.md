# Public API regression tests

`supplied_api.rs` rejects a coordinator belonging to another runtime through
public APIs. Native tool requests, attachment lifetime, continuing sessions and
host hooks are covered by module tests; the standalone example additionally
captures the topology and configuration isolation scenarios.
