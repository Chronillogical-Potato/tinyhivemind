# Offline host fixtures

`mod.rs` supplies a runtime configuration with local background services disabled
and a loopback backend for incidental calls. The standalone example owns the
model responder and captures actual requests. `test.rs` checks configuration
and backend startup. This module is enabled by the `offline` Cargo feature.
