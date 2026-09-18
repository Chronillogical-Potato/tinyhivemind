# Sandbox internals

| File | Purpose |
| --- | --- |
| `cleanup.rs` | Force-removes Docker containers with a short deadline and independently verifies their absence. |
| `preflight.rs` | Bounds daemon readiness capture, assigns a unique preflight name/cidfile, validates confinement, and always cleans up. |
| `test_support.rs` | Exposes configurable sandbox limits to the test suite without expanding the runtime module. |
