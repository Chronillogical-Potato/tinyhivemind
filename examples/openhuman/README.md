# Embedded OpenHuman routing proof

This example runs the real TinyHiveMind `JevRouter` composition and then hands
the accepted route to one embedded OpenHuman agent.

It is deterministic and offline:

- a `SystemOneTransport` fixture returns typed Choice and Noul answers to the
  exact questions built by `JevRouter`;
- a loopback mock serves OpenHuman's incidental backend calls and its
  OpenAI-compatible model call;
- one ephemeral, read-only OpenHuman `Harness` runs the selected seat with a
  stable per-agent session id;
- no credential, network provider, inherited workspace, or user data is used.

Run the standalone example from the repository root:

```sh
cargo run --manifest-path examples/openhuman/Cargo.toml
```

Expected output includes the accepted `engineering` route, exactly one System
One request, the stable session id, and the mock reply `openhuman-seat-ok`.

This proves integration mechanics, not Jev routing quality or provider
performance. Live TypeSafe quality remains the job of the labeled routing
corpus and paid campaign described in
[`docs/specs/jev-first-routing.md`](../../docs/specs/jev-first-routing.md).

## Files

| File | Purpose |
| --- | --- |
| `Cargo.toml` | Standalone dependency boundary, outside the library workspace and MSRV contract. |
| `src/main.rs` | System One fixture, route composition, embedded Harness, and assertions. |
